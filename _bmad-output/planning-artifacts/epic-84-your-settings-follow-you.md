# Epic 84 — Your settings follow you

created: '2026-09-24'
source: the owner's question of 2026-09-24 (verbatim below), asked after syncing the account's config repository, and the owner's three answers to the coordinator's follow-up the same day. Four read-only lanes grounded it in the repository at `6d211de` (the merge of epic 83's surface rung): `local://g84-GSettings.md`, `g84-GRepo.md`, `g84-GDrives.md` and `g84-GBotsMatrix.md`. The coordinator froze the model as `local://epic84-contract.md`, then amended it during the build wave: A1, A2' (which superseded A2) and A3, all below. Line numbers are at `6d211de`. Where a digest's number had drifted, this plan re-read the line and cites what it found.
binds: FR-713…FR-722 and NFR-97…NFR-98 (allocated here); AD-320…AD-325; UX-DR118; DW-300…DW-305 (allocated in *What stays out*); D-26 (drafted at the end, for `docs/decisions.md`). **FR-712, NFR-96, AD-319, UX-DR117, DW-299 and D-25 were the previous ceilings** (epic 83; D-25 is `docs/decisions.md:1247`). A repo-wide grep of `_bmad-output`, `docs`, `src` and `src-tauri/crates` for `epic-84`, FR-713…FR-722, NFR-97, NFR-98, AD-320…AD-325, UX-DR118, DW-300…DW-309 and D-26 found nothing before this plan.
see-also: epic 82 (the account, the config repository, AD-309's live account tiers, AD-312's create-only rule that AD-324 narrows, AD-314's device rename, AD-315's credential choice, D-25); epic 83 (the *Add account* menu and the Matrix login overlay); epic 46 (AD-98: the file wins); AD-27 (absent rather than disabled); AD-40 (the crate split); AD-62 (no timers in Rust); `docs/account.md` § *Your settings, drives and accounts travel*.

## The owner's ask

Verbatim:

> great is all the settings will be reflected in the file on the sync? (im not seeing after sync the settings url) - if settings exists pull it if not take from other device of the user, if this is the first device create from template.

In other words: every setting should be in a file in the config repository, and a sync should keep that file current. After a sync, the owner found no settings in the repository. When the file exists, a device pulls it. When it does not, the device takes the settings from another of the person's devices. On the person's first device, the file is created from the template.

The owner answered the coordinator's three questions on 2026-09-24:
1. **Synced and editable.** New files sit beside the pins. Editing a setting in the app writes it back and pushes it. `keeper.toml` stays the optional lock.
2. **Carry all of it:** drives, bot providers, Matrix accounts and the preference keys.
3. **A new device's machine-local settings** come from the **latest device of the same class**, or else from the class template.

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| Settings in the repository after a sync | **absent** | Nothing writes a setting's value to the config repository. `layout::plan` is create-only: its `create` closure emits a write only when the file is absent (`keeper-core/src/org_account/layout.rs:173-211`, closure `:180-188`). The transport refuses an existing path with "files there are never rewritten" (`keeper-sync/src/config_repo.rs:209-219`). The only values in `<login>/` are the two template copies (`layout.rs:190-192`, `:201-204`). A value set in the pane lands only in keeper.db (`keeper-core/src/registry.rs:230-238`). No `settings.toml` exists anywhere in the tree (GSettings, closing note). |
| One writer for every setting | **present** | `registry::set_setting` (`registry.rs:230`) and the private `delete_setting` (`registry.rs:1735`) are the only writers. The coverage test scans four crates for every call site (`config/keys.rs:1364-1454`). About 50 typed setters and every settings IPC command land there (GSettings §1). |
| Being told when a setting changes | **absent** | There is no settings-changed event in either direction: grep for `settings_changed`, `SettingsChanged` and `config_changed` finds nothing (GSettings §4). `set_setting` is keeper-core and has no `AppHandle`. |
| Which keys may travel | **present, five need translation** | `KeySpec.scope` classifies every key (`config/keys.rs:349-948`). `UserGlobal` values are portable as they stand. Four `MachineLocal` or `UserGlobal` keys hold a local id: `notes.active_vault` (`keys.rs:542`), `tasks.ledger_vault` (`:552`) and `recording.destination_profile_id` (`:700`) name a sync profile, for the reason `PROFILE_ID_WHY` gives: "the same folder is a different profile id on the other machine" (`keys.rs:956-958`). `bots.voice_target` (`:458`) names a bot. `notes.embedding_model` (`:592`) holds a provider id inside its JSON. Two never travel: `sdk_encryption` (`:352`, keyed to this machine's keychain passphrase) and `sync.git_path` (`:825`, an executable on this disk). |
| A value that travels without becoming a pin | **present, if it goes into the table** | `get_setting` asks `config::setting_override` first and reads the table only when no layer sets the key (`registry.rs:208-227`). The layer order puts the account tiers above `~/.keeper` (`config/mod.rs:133`). A value written into `<login>/keeper.toml` would therefore shadow the pane on every read (the test at `registry.rs:5159-5188`). A value applied into the `settings` table stays editable wherever no layer speaks. |
| Rewriting a file in `<login>/` | **absent, by design** (AD-312) | The transport refuses an existing entry (`config_repo.rs:209-219`) and a file ancestor (`refuse_non_directories_on_the_way`, `:399-423`). `editor.upsert` already replaces a blob, so an overwrite needs only the refusal relaxed, while both symlink refusals and `tree_path` (`:797-823`) stay (GRepo §1). |
| Which device changed a file last | **half-present** | A device record carries only `created`, and `DeviceEntry` surfaces no timestamp (`layout.rs:342-349`). Commit subjects name a device only for a registration or a rename (`keeper/src/account_ipc.rs:1056`, `:1557`). `git::history::file_log(repo, rel, limit)` returns the newest commits that touched a path, with `committed_secs` (`keeper-sync/src/git/history.rs:139`, `FileRevision` `:38-52`). It does not follow renames (`:14-17`). |
| A drive described for another device | **present, as a taxonomy** | `FOLDER_FIELD_RULES` classifies every `SyncProfile` field as Allowed, Identity or MachineLocal (`keeper-sync/src/profile/folder.rs:215-263`; the profile `profile/mod.rs:933-1213`). Ids are minted per machine (`keeper/src/sync_ipc.rs:839-857`). There is no duplicate guard: "`db::upsert_profile` has no duplicate-path guard" (`sync_ipc.rs:1342-1343`), and none for remotes either. The token lives in the keychain at `sync/<id>/credential` (`profile/mod.rs:1390-1392`). The account-or-keychain choice is `sync.credential_source.<pid>`, which is `SessionState` and refused from files (`keys.rs:837-841`). |
| Bot providers and bots on another device | **present, local only** | `bot_providers` and `bots` are keeper.db tables (`keeper-core/src/bots/store.rs:81-118`). "No credential column exists here, and none can be added" (`store.rs:21`). The provider token is `bot_provider_token/{id}` (`bots/mod.rs:382-384`). `base_url` is stored normalized by `parse_base_url` (`keeper/src/bots_ipc.rs:413-415`). The account opt-in is `bots.provider_credential_source.<id>`, which is `SessionState` and refused from files (`keys.rs:402-406`). |
| Matrix accounts on another device | **present, local only** | The `accounts` table holds `user_id`, `homeserver_url` and the login-mechanism tag `provider` (`registry.rs:87-95`, `:483-487`). The session is the keychain item `session/{account_id}` (`keeper-core/src/auth.rs:397-399`), and the store passphrase is `store_passphrase/{account_id}` (`auth.rs:404-410`). Neither can travel, and a new device logs in afresh. |
| A prefilled Matrix login | **absent** | `LoginScreenProps` carries only `addMode` and `onDone` (`src/components/auth/login-screen.tsx:88-97`). Every field starts from `useState("")` (`:157-159`, `:356-357`). The login commands already take the homeserver and the username as parameters (GBotsMatrix §2). |
| An "on your other devices" surface | **absent** | No export exists for providers or accounts (GBotsMatrix §4). The config repository "knows nothing of what the files mean" (`config_repo.rs:3-7`). The mounts an offer would join: the Sync section's drive list, the Bots section's provider list (`src/components/settings/bots-section.tsx:471-501`, mounted at `settings-dialog.tsx:256`), and the Matrix overlay (`src/App.tsx:372`, and the first-run step `first-run-wizard.tsx:331-365`). |
| A sync without a timer | **present** | Triggers are launch (`account_ipc.rs:641`), window focus (`src/hooks/use-account-mirror.ts:71-89`) and *Sync now* (`src/components/settings/account-section.tsx:250-254`). An unforced sync runs at most every 15 minutes (`account_ipc.rs:69`, the check `:1123-1128`) and never beside another (`gate.try_lock`, `:1130-1132`). |

## The one sentence

**keeper writes a person's settings into the repository only when their directory is created, then never again. It keeps drives, bots and Matrix accounts on the device that added them, and cannot tell which device changed a file last. The fix: two settings files merged key by key into the same table the pane writes (so pins still win), local ids that travel as references, a new device seeded from the most recently changed device of its class, three manifests of drives, bot providers and Matrix accounts that are offered and never added by themselves, exactly five files keeper may rewrite, and a hook on the one writer that kicks a sync whenever one of them changes.**

## What earlier epics decided, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **AD-312 / FR-690** (epic 82) | writes are "create-only"; "a file that exists is never re-copied or rewritten" | A settings file that follows every edit. | **Narrowed (AD-324).** Exactly five files directly under `<login>/` may be replaced: `settings.toml`, `settings.<device>.toml`, `drives.toml`, `bots.toml` and `matrix.toml`. `user.toml`, `keeper.toml`, `keeper.<device>.toml` and `devices/*.toml` stay create-only. |
| **D-25** | keeper refuses "rewriting a file that exists, their own `user.toml` included" | The same five files. | **Narrowed, and recorded as D-26.** Only in the person's own directory. Another person's directory is still never written. |
| **AD-309** (the account tiers are live layers above `~/.keeper`) | a key set in `<login>/keeper.toml` wins on every read | Preferences that travel and stay editable. | **Held.** The settings files are not layers. Their values are applied into keeper.db's `settings` table, below every layer. GSettings §2 recommended writing synced values into `keeper.toml`. That is rejected, because every travelling preference would become a pin that shadows the pane. |
| **AD-315** (`keys.rs:402-406`, `:837-841`) | the credential-source families are `SessionState` and `Settable::Never`: "a file flipping it would send the account's token to a remote the person never chose" (`docs/settings-keys.md:171`) | A drive or provider that says it uses the account. | **Held.** No file sets either family. A manifest records only the *choice* (`account`, `own` or `none`) as a description. A prefilled form preselects it, and the person confirms. `account_offer_add_provider` sets it only on an explicit *Add*. |
| **`PROFILE_ID_WHY`** (`keys.rs:956-958`) | a profile id is refused from shared files | Vault and destination choices that follow the person. | **Held for layer files.** The settings files carry a reference (AD-321), never the id. |
| **AD-314** (the rename moves both of a device's files) | `plan_rename` moves `devices/<from>.toml` and `keeper.<from>.toml` (`layout.rs:391-436`) | A third per-device file. | **Extended (A3).** It also moves `settings.<from>.toml`, and the device's base survives the rename. |
| **FR-702** (launch, focus at most every 15 minutes, *Sync now*) | an unforced sync is throttled | An edit that reaches the repository without waiting. | **Extended (AD-325).** A change to something that travels marks the account dirty, and a dirty sync bypasses the throttle. There is still no timer (AD-62). |
| **AD-40** | keeper-core never depends on keeper-sync, nor the reverse | Drive facts from sync.db. | **Held.** keeper-core cannot see sync.db, so the shell builds the drive catalog and hands it in. |

## Decisions this epic takes

The rules below are the plan. The coordinator amended the contract during the build wave (*Contract amendments*, below), and the rules already include those amendments.

- **AD-320: Synced settings are values in two files, merged per key.**

  **Binds:** FR-713, FR-714, FR-715; NFR-98; Story 84.1; D-26.

  **Prevents:**
  - a preference that becomes a pin because it travelled, so that its control turns into *Set by a file*;
  - a disk-bound, keychain-bound or session key leaving the device;
  - a key written by a newer build being lost when an older build rewrites the file;
  - a sync that silently undoes a change made here since the last sync;
  - a whole-file last-writer-wins that loses another device's unrelated change;
  - a device that cannot apply a value deleting it for every other device.

  **Rule:**
  - **The files**, directly under `<login>/`:
    - `settings.toml` holds every key where `synced_file(key) == Some(Shared)`;
    - `settings.<device>.toml` holds every key where `synced_file(key) == Some(Device)`.
  - **The format.** TOML, rendered deterministically: sorted keys and a two-line header comment.

    ```toml
    # keeper keeps this file in step with your devices. Edit it freely; keeper merges it key by key.
    # Values here are preferences, not locks — keeper.toml is where a setting is pinned.
    [settings]
    "recording.codec" = "hevc"
    "notify.previews_enabled" = true
    ```

    A value is written with `Shape::to_toml` (new, the inverse of `Shape::coerce`) and read with `Shape::coerce`. A key this build does not know, or one that belongs in the other file, is **preserved verbatim** and never applied.
  - **`synced_file(key)`** (keeper-core `org_account::settings_sync`):
    - `Shared`: the spec is not a family and `scope == UserGlobal`. This includes `notes.embedding_model` and `bots.voice_target`, which are translated (AD-321);
    - `Device`: the spec is not a family, `scope == MachineLocal`, and the key is neither `sdk_encryption` nor `sync.git_path`;
    - `None`: everything else, meaning `SessionState`, every family, and unknown keys.
  - **`merge`** is pure (`settings_sync::merge`). Its inputs:
    - `remote: Option<Values>`: the file at the fetched tip, `None` when absent;
    - `seed: Option<(Values, FirstSync)>`: used only when `remote` is `None`;
    - `local: Values`: this device's table rows for the file's keys, translated to portable form;
    - `base: Option<Values>`: the file as this device last synced it, `None` when never;
    - `resolves: &dyn Fn(&str, &str) -> bool`: whether a portable value can be applied on this device. It decides what "applied" means (A2'). The shell passes `|k, v| from_portable(k, v, &catalog).is_some()`.
  - **Which case applies:**
    - `remote == None`: `R` is the seed's values, or empty. It is a first sync with the seed's precedence, or `LocalWins` when there is no seed;
    - `remote` present and `base == None`: a first sync, `RemoteWins`. **If the settings exist, pull them.**
  - **A first sync**, per key in R ∪ L:
    - in both: `RemoteWins` writes r to the file and applies r when r ≠ l; `LocalWins` writes l;
    - only in R: the file gets r, and r is applied;
    - only in L: the file gets l.
  - **A later sync**, per key in B ∪ R ∪ L, where a missing side is `None`:
    - `l == b`: the file gets r, and r is applied (a set, or a delete when r is `None`) when r ≠ l;
    - `l != b` and `r == b`: the file gets l (a push);
    - `l != b` and `r != b`: the file gets l. The device syncing now wins (D-26).
  - **A value that cannot be applied here (A2').** Whichever case above would apply the remote value r, when `resolves(key, r)` is false the key is **not** applied and the file keeps r. Both bases then record this device's own value l for that key, `None` included, and not r. The key is therefore never dirty, so this device neither pushes its deletion nor pushes its own value back over it. When the value later resolves here, l == b holds and r is applied at that sync.
  - **The output, `Merged`:**
    - `file: Values`;
    - `apply: Vec<(String, Option<String>)>`: stored spellings, already translated back to local ids. A key whose value does not resolve here is left out of `apply` and kept in `file`;
    - `changed: bool`: the file differs from the remote, including when the remote is absent;
    - `base_if_pushed: Values`: equal to `file`, except that a key not applied because its value does not resolve here records l (A2');
    - `base_if_not_pushed: Values`: B with the pulled keys set to r, the keys not applied set to l, and the pushed keys left at b, so they stay dirty.
  - **The base** is stored in keeper.db under `account.<id>.settings_base.shared` and `account.<id>.settings_base.device`, as a JSON object. The `account.` family already covers both keys (`SessionState`, `Settable::Never`). *Forget this account* clears them.
  - **Where values land.** Applied values go into keeper.db's `settings` table, the place the pane writes. Every layer, `keeper.toml` and `keeper.<device>.toml` included, still shadows them on read (`registry.rs:208-227`).

- **AD-321: Local identifiers travel as references.**

  **Binds:** FR-716; Story 84.2.

  **Prevents:**
  - one device's sync.db ULID applied on another (`keys.rs:956-958`);
  - a voice target, or an embedding model's provider, naming a row this device does not have;
  - two spellings of one remote treated as two drives;
  - a portable preference dropped because its drive or bot is not on this device yet;
  - another device's recording folder applied to a disk that lacks it (A1).

  **Rule:**
  - **The portable forms**, as TOML strings:
    - **drive:** `drive:<normalized remote_url>#<branch>`, used by `notes.active_vault`, `tasks.ledger_vault` and `recording.destination_profile_id`;
    - **provider:** `provider:<kind>:<normalized base_url>`, used inside `notes.embedding_model`'s JSON: `{"provider": "provider:…", "model": "…"}`;
    - **bot:** `bot:<kind>:<normalized base_url>#<target>`, used by `bots.voice_target`.
  - **`normalize_remote(url)`:**
    - lower-cases the scheme and the host;
    - removes a trailing `/` and a trailing `.git`;
    - strips userinfo;
    - leaves an `ssh` or scp form as it is, apart from trimming `.git`.
  - **`normalize_base_url`** reuses the bots' `parse_base_url` normalisation, in keeper-core.
  - **`Catalog { drives: Vec<(profile_id, DriveRef)>, providers: Vec<(provider_id, ProviderRef)>, bots: Vec<(bot_id, provider_id, target)> }`.** The shell builds `drives`. keeper-core builds providers and bots from `bots::store` itself (`Catalog::with_bots(data_dir, drives)`).
  - **`to_portable(key, stored, &Catalog) -> Option<String>`** is `None` when the local id is unknown, and the key is then left out of L.
  - **`from_portable(key, portable, &Catalog) -> Option<String>`** is `None` when the reference does not resolve here, and the value is then not applied.
  - **A path applies only where it exists (A1).** For a key whose shape is `Shape::AbsolutePath` (`recording.destination_dir`), `from_portable` is `None` unless the value names an existing directory on this device. The value stays in the file, is not applied, and is not a local change (A2').
  - Every other key passes through unchanged.

- **AD-322: A new device's files are seeded.**

  **Binds:** FR-717; Story 84.3.

  **Prevents:**
  - a new Mac starting with default hotkeys when the person already has a Mac;
  - a phone inheriting a desktop's hotkeys;
  - a seed chosen by device name rather than by recency;
  - a first device's local values overwritten by a template;
  - a new "last updated" field in every device record. Git history already knows when each file last changed, and `DeviceEntry` carries no timestamp (`layout.rs:342-349`).

  **Rule:**
  - **`settings.<me>.toml` is absent:**
    - Seed from the same-class device, among `layout::devices` with slug ≠ me, whose `settings.<slug>.toml` exists and changed most recently. Recency comes from the `last_change` closure: `None` is oldest, and ties go by slug. That seed wins (`RemoteWins`): **take it from another device**;
    - otherwise, `_template/settings/<class>.toml` with `LocalWins`;
    - otherwise, no seed.
  - **`settings.toml` is absent:** `_template/settings.toml` with `LocalWins`, otherwise no seed.
  - The functions are `settings_sync::seed_device(files, login, class, me, last_change: &dyn Fn(&str) -> Option<i64>) -> Option<(Values, FirstSync)>` and `seed_shared(files) -> Option<(Values, FirstSync)>`.
  - The shell's `last_change` is `keeper_sync::config_repo::last_change_secs(clone_dir, rel)`.
  - **A rename is not a new device (A3).** `plan_rename` moves `settings.<from>.toml` with the device's other two files, and the device base is kept. A renamed device therefore finds its own file and is never re-seeded.

- **AD-323: Drives, bot providers and Matrix accounts travel as offers.**

  **Binds:** FR-718, FR-721, FR-722; NFR-97; Stories 84.4, 84.6; UX-DR118.

  **Prevents:**
  - a drive added with no folder chosen, which would clone into a path nobody picked;
  - a Matrix session restored from a file (the session is secret, `auth.rs:397-399`);
  - a provider added with no key;
  - a token or a keychain value in the repository;
  - a record written by a newer build losing its fields in an older build;
  - one device's removal taking a drive off another device's list.

  **Rule:**
  - **The files**, under `<login>/`:
    - `drives.toml`, with `[[drive]]` tables;
    - `bots.toml`, with `[[provider]]` tables and nested `[[provider.bot]]` tables;
    - `matrix.toml`, with `[[account]]` tables.

    They are rendered with serde and toml, sorted by identity, each with a header comment. Unknown fields are preserved (`#[serde(flatten)] extra: BTreeMap<String, toml::Value>`).
  - **The records** (keeper-core `org_account::manifest`, serde `snake_case`):
    - `DriveRecord`: `name`, `remote_url`, `branch`, `credential` (`"account"`, `"own"` or `"none"`), the role subfolders `notes`, `recordings`, `sessions` and `tasks` (`Some` means the role is on), `excludes`, `lfs_threshold_bytes`, `virtual_patterns`, `virtual_over_bytes`, `release_ttl_ms`, `tags`, `commit_subject_template`, `devices` and `extra`. These are the fields `FOLDER_FIELD_RULES` calls Allowed, plus the identity (`folder.rs:215-263`). The local path, direction, lane, subpaths, LFS mode, cadence and author override stay on each device;
    - `ProviderRecord`: `kind`, `name`, `base_url`, `credential` (`"account"` or `"own"`), `read_timeout_ms`, `bots`, `devices` and `extra`;
    - `BotRecord`: `target`, `name`, `pin_order`, `shape`, `colour`, `mark` and `extra`. Grants are device-local and do not travel (`store.rs:150-165`);
    - `MatrixRecord`: `user_id`, `homeserver_url`, `kind` (`"password"`, `"oidc"` or `"beeper"`), `devices` and `extra`.
  - **Identity:**
    - a drive is `(normalize_remote(remote_url), branch)`;
    - a provider is `(kind, normalize_base_url)`;
    - a Matrix account is `user_id`.
  - **`manifest::merge_*(remote: &[T], mine: &[T], device: &str) -> Vec<T>`**, where `mine` is this device's current entries with `devices` empty:
    - an entry this device has takes its fields from `mine` (last writer wins) and gains `device` in `devices`;
    - a remote entry this device lacks loses `device` from `devices`;
    - an entry whose `devices` becomes empty is dropped;
    - the output is sorted by identity.
  - **`manifest::offers(remote_drives, remote_providers, remote_matrix, mine…) -> AccountOffersVm`** returns the remote entries whose identity is not in `mine`.
  - **Nothing is added automatically.** A drive needs a folder, a Matrix account needs a sign-in, and a provider needs a key unless it uses the account.
  - **The VMs** (`state.rs`, ts-exported, camelCase):
    - `AccountOffersVm { drives, providers, matrix }`;
    - `DriveOfferVm` has the drive record's portable fields plus `key` and `devices`;
    - `ProviderOfferVm { key, kind, name, baseUrl, credential, bots /* bot names */, devices }`;
    - `MatrixOfferVm { key, userId, homeserverUrl, kind, devices }`.

    `key` is the identity rendered as a string: the AD-321 reference form for drives and providers, and `matrix:<user_id>` for Matrix. `AccountVm` gains `offers`, empty by default (`AccountFacts.offers`).

- **AD-324: The repository may rewrite exactly these files.**

  **Binds:** FR-719; NFR-97; Stories 84.1, 84.3; D-26.

  **Prevents:**
  - rewriting `user.toml`, a pin file (`keeper.toml`, `keeper.<device>.toml`) or a device record;
  - writing through a symbolic link, over a directory or under a file;
  - writing into another person's directory or into `_template/`;
  - a create that silently overwrites.

  **Rule:**
  - **keeper-core `layout::is_rewritable(login, rel) -> bool`** holds when `is_own_path` holds, `rel` sits directly under `<login>/`, and the file name is `settings.toml`, `settings.<valid slug>.toml`, `drives.toml`, `bots.toml` or `matrix.toml`.
  - **keeper-sync `config_repo::Write`** gains `pub replace: bool`:
    - `false` stays create-only, as today;
    - `true` may overwrite an existing **regular** file;
    - a symbolic link, a directory, a non-directory ancestor, and every `tree_path` refusal are still refused either way;
    - every existing constructor passes `replace: false`.
  - **`config_repo::last_change_secs(dir: &Path, rel: &str) -> Result<Option<i64>>`** returns the time of the newest commit that touched `rel`. It wraps `git::history::file_log(dir, rel, 1)`.
  - **The commit message** is `"{login}: settings from {device}"`, also when only the manifests changed.

- **AD-325: Write-back is a hook on the one writer.**

  **Binds:** FR-715, FR-720; NFR-98; Story 84.5.

  **Prevents:**
  - about 50 typed setters each having to remember to kick a sync. GSettings' recommendation notes that that layer would also miss the writers in `auth.rs` and `archive/mod.rs`;
  - a timer in Rust (AD-62);
  - a loop, where applying pulled values kicks another sync;
  - a burst of edits becoming a burst of pushes;
  - a change lost because a sync was already running;
  - any side effect without an account.

  **Rule:**
  - **keeper-core `registry`:**
    - `pub fn set_setting_observer(f: Box<dyn Fn(&str) + Send + Sync>)` stores one process-wide observer in a `OnceLock`. A second call is ignored;
    - `set_setting` and the private `delete_setting` call it with the key after a successful write, **unless** suppressed;
    - `pub fn with_observer_suppressed<T>(f: impl FnOnce() -> T) -> T` uses a thread-local depth counter;
    - the coverage test (`keys.rs:1364-1454`) stays green.
  - **The shell:**
    - at boot it installs an observer that calls `account_ipc::note_local_change()` when `synced_file(key).is_some()`;
    - `note_local_change()` sets `RUNTIME.dirty = true` and spawns `sync(force = false)`. It is a no-op without a configured account;
    - in `sync`, a set `dirty` flag bypasses the 15-minute throttle. When `gate.try_lock` fails, the flag stays set. When a converge finishes and `dirty` was set during it, one more sync runs. This coalesces edits without a timer;
    - `note_local_change()` is also called after a successful `sync_profile_save` or remove, `bots_provider_save` or remove, `bots_bot_save` or remove, `bots_provider_credential_source_set`, `sync_credential_source_set`, and after a Matrix account is added (on every login success path) or removed.
  - **The sequence**, inside `converge`, after `apply_clone` resolves `Mine` and the registration push, and before the final `apply_clone`:
    1. **Build the catalog.**
       - Drives: the engine's profile list gives `(id, DriveRef)`, plus a `DriveRecord` for `mine`. The credential is `account` when `sync.credential_source.<pid>` names this account, `own` when a `sync/<pid>/credential` secret exists, and `none` otherwise. Roles come from the profile's blocks.
       - Providers and bots: `bots::store` gives `ProviderRecord`s. The credential is `account` when `bots.provider_credential_source.<id>` is the account, and `own` otherwise.
       - Matrix: `registry::list_accounts` gives `MatrixRecord`s. The `provider` column gives the kind, with `None` read as `"password"`.
    2. **Build `local`** for each settings file from the `settings` table's rows for synced keys (the rows, **not** the layer-resolved values), translated by `to_portable`.
    3. **`commit_and_push`** with a plan closure that, on **every** attempt:
       - reads R from the worktree;
       - computes the seeds, using `last_change_secs`;
       - runs `merge` for both files and `merge_*` for the three manifests;
       - emits `Write { replace: <the file exists> }` for every file whose rendered bytes differ from the worktree;
       - filters every write through `layout::is_rewritable`;
       - keeps the last attempt's results in a `Mutex`.
    4. **After the push:**
       - `Pushed` or `NothingToDo`: base := `base_if_pushed`;
       - a failed push: base := `base_if_not_pushed`;
       - either way, `apply` runs **under `with_observer_suppressed`**. A key with a typed setter that keeps in-memory state goes through that setter: notification previews, do-not-disturb, the dock badge, the menu bar, incognito and bots' message details through the `Accounts` facade; `debug.mode` through `debug_log::set_enabled`; the voice-wake trio through the voice runtime's apply path. Every other key goes through `registry::set_setting`, or is deleted;
       - a key read only at launch, where no live setter exists (the hotkeys), applies at the next launch.
    5. **Offers** are computed from the post-push manifests against `mine`, stored in `inner.offers` and published.
    6. **Offline** (the fetch failed): nothing is merged. Local changes stay dirty, because the base is unchanged, and are pushed at the next successful sync.
    7. **`NotMine` or `Blocked`:** nothing is merged or pushed.
  - **A new command, registered on every target:** `account_offer_add_provider(key: String) -> Result<(), IpcError>`.
    - It finds the provider offer by `key`.
    - It refuses with a sentence unless the offer's credential is `account` and the account is usable. The frontend then opens the prefilled form instead.
    - It inserts the provider, sets its credential source to the account, and inserts its bots (targets and names, in pin order). Then it calls `note_local_change()`.

- **UX-DR118: "From your account".**

  **Binds:** AD-323; FR-721, FR-722; Story 84.6.

  **Prevents:**
  - an empty block, or a disabled one (AD-27);
  - a drive added with no folder chosen;
  - a provider that needs a key being added without one;
  - a Matrix offer that signs in by itself;
  - a separate screen for offers, away from the lists they belong to.

  **Rule:**
  - Offers come from `accountStore.vm.offers`. Every "from your account" block is **absent** when its list is empty.
  - **Settings › Sync**, in the section that lists drives: a block titled `From your account` with the sentence `Drives you use on your other devices. Add one to sync it here too.`
    - One row per drive offer: the name, the host of `remoteUrl` (mono), the roles as chips, and `on <devices joined by ", ">`.
    - `Add…` opens the existing add-folder form **prefilled** with the name, remote URL, branch, role switches and subfolders, and the portable policy fields.
    - The account credential is preselected when `credential === "account"` and the account is usable. The person picks the folder on desktop. iOS assigns it, as today.
  - **Settings › Bots**, in the providers list: a block titled `From your account`. One row per provider offer: the name, the kind, the host and the bot names.
    - `credential === "account"` and the account is usable: `Add` calls `accountOfferAddProvider(key)`. A refusal sentence shows inline.
    - Otherwise: `Add…` opens the provider form prefilled with the kind, name and base URL, and asks for its key as today.
  - **LoginScreen**, in add mode and in the first-run step: when Matrix offers exist, a block above the tabs titled `From your account`, with one button per offer: `{userId}` plus the host in muted text.
    - A `password` or `oidc` offer prefills the homeserver and the username on the Password tab. A `beeper` offer selects the Beeper tab.
    - `LoginScreen` handles the optional `prefill` internally. Nothing is persisted.
  - **Settings › Account**, signed in: one line under the status.
    - `Your settings sync with this account. Last synced {time}.` The time reuses the status's time; without one, the second sentence is omitted.
    - When offers exist, it continues with `On your other devices: {n} drive(s), {n} bot provider(s), {n} Matrix account(s).` A count of zero is omitted, and each noun takes its correct plural.
  - **`dev/mock-shell.ts`:** `?account=ready` carries two drive offers, one account-credential provider offer with two bots, and one Matrix offer. `account_offer_add_provider` removes that offer.
  - Every `OrgAccountVm` literal gains `offers: { drives: [], providers: [], matrix: [] }`, including `NO_ACCOUNT`, `src/test/account-fixture.ts` and the mock shell.

### Alternatives this plan rejected

- **Synced values as `keeper.toml` pins** (GSettings §2). Every travelling preference would shadow the pane and read *Set by a file*. That contradicts "synced and editable" (NFR-98).
- **One file, whole-file last-writer-wins.** Two devices that change two different keys between syncs would lose one change.
- **A `last_updated` field in `devices/<d>.toml`.** It would mean rewriting a device record on every edit, and git history already answers the question (`history.rs:139`).
- **Commit trailers naming the device** (GRepo §5). Neither the merge nor the seed needs *which* device made a change. The seed needs *when* the file changed, and the file name already says whose file it is.
- **Adding offered drives, providers or accounts automatically.** Each needs something only the person can give: a folder, a key or a sign-in.
- **A drive id stored in the repository** (GDrives §3, option b). A travelling file that re-points a clone is what `FolderFieldRule::Identity` exists to stop (`folder.rs:216-220`). The normalized remote and branch are identity enough.
- **A hook in every typed setter.** That is about 50 edits, and it misses the core-internal writers (GSettings, *Recommendation*).
- **A debounce timer for write-back.** AD-62 forbids timers in Rust. The dirty flag and the single rerun coalesce edits without one.

## Contract amendments

The coordinator amended the frozen contract four times during the build wave. A2' superseded A2 on the same day, so three amendments stand. The code is built to them, and where the contract and an amendment disagree, the amendment wins.

- **A1: A path applies only where it exists.** `recording.destination_dir` stays in `settings.<device>.toml`, because the owner wants a new Mac to start from the last Mac. A value whose key has `Shape::AbsolutePath` is applied only when that directory exists on this device: `from_portable` returns `None` otherwise. The value stays in the file, is not applied, and is not a local change.
- **A2' (it supersedes A2): a value that cannot be applied here is never a local change.**
  - **The rule.** For any key where `merge` would apply the remote value r, but `resolves(key, r)` is false:
    - the key is not applied, and the file keeps r;
    - both `base_if_pushed` and `base_if_not_pushed` record the local value l for that key (`None` included), not r.
  - **What follows:**
    - the key is never dirty and never pushes a deletion;
    - two devices that each cannot resolve the other's value do not push it back and forth;
    - when the reference resolves later, l == b holds and r is applied then.
  - **The signature.** `merge` takes `resolves: &dyn Fn(&str, &str) -> bool`, which decides what "applied" means. The shell passes `|k, v| from_portable(k, v, &catalog).is_some()`.
  - **What it replaced.** A2 had taken l to be b only when l was `None`. That stopped a deletion, but it left a flip-flop when l was `Some`. The case: device B has `bots.voice_target = Y`, device A has X, and neither has the other's bot. Each would push its own value over the other's at every sync, one commit each time.
- **A3: A device rename moves its settings file too.** `layout::plan_rename` also moves `<login>/settings.<from>.toml` to `settings.<to>.toml` when it exists, under the same refusal rules as the other two files. `account_rename_device` keeps `account.<id>.settings_base.device` as it is, because the base belongs to the device and not to its name. There is no re-seed after a rename.

## Requirements allocated here

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-713 | Every preference key keeper knows and may carry lives in one of two files in the person's directory. `<login>/settings.toml` holds every user-global key that is not a family. `<login>/settings.<device>.toml` holds every machine-local key that is not a family, except `sdk_encryption` and `sync.git_path`. Session-state keys and families never travel. Each file is TOML with a `[settings]` table, sorted keys and a two-line header saying the values are preferences, not locks. A key this build does not know, or one that belongs in the other file, is kept in the file verbatim and never applied. | 84.1 | AD-320 |
| FR-714 | The two files are merged key by key. A device's first sync against an existing file takes the file's values. At a later sync, a key unchanged here since the last sync takes the file's value (a delete included). A key changed only here is pushed. A key changed both here and in the file keeps this device's value. A value this device cannot apply stays in the file and is not a change made here. This device therefore never deletes it or pushes its own value back over it, unless the person changes that setting here. | 84.1 | AD-320 |
| FR-715 | A synced value is applied into this device's settings table, the same place the Settings pane writes. It takes effect as soon as the sync ends, through the setting's live setter where one exists. A key read only at launch (the hotkeys) takes effect at the next launch. A setting pinned by any layer file, `keeper.toml` and `keeper.<device>.toml` included, still wins on read. | 84.1, 84.5 | AD-320, AD-325 |
| FR-716 | A setting that names a drive, a bot provider or a bot travels as a reference: `drive:<remote>#<branch>`, `provider:<kind>:<base url>` or `bot:<kind>:<base url>#<target>`, with the remote and base URL normalized. A reference that does not resolve on this device is kept in the file and not applied. An absolute path is applied only when it names an existing directory on this device. | 84.2 | AD-321 |
| FR-717 | When this device's `settings.<device>.toml` is absent, it is seeded from the device of the same class whose file changed most recently in the repository, and that device's values win. With no such device, it is seeded from `_template/settings/<class>.toml`, and this device's own values win. When `settings.toml` is absent, it is seeded from `_template/settings.toml`, and this device's values win. A renamed device keeps its file and is not re-seeded. | 84.3 | AD-322 |
| FR-718 | `<login>/drives.toml`, `bots.toml` and `matrix.toml` list the drives, the bot providers with their bots, and the Matrix accounts the person uses, each with the devices that use it. A drive is identified by its normalized remote and branch, a provider by its kind and normalized base URL, and a Matrix account by its user id. Removing one on a device removes that device from its list, and an entry no device uses is dropped. Fields a newer build wrote are preserved. | 84.4 | AD-323 |
| FR-719 | keeper rewrites exactly five files, and only in the signed-in person's own directory: `settings.toml`, `settings.<device>.toml`, `drives.toml`, `bots.toml` and `matrix.toml`, directly under `<login>/`. A symbolic link, a directory or a file ancestor at one of those names is refused. Every other file stays create-only. Each such commit reads `{login}: settings from {device}`. | 84.1 | AD-324 |
| FR-720 | Changing a synced setting, or adding or removing a drive, a bot provider, a bot or a Matrix account, starts a sync that is not held back by the 15-minute throttle. Changes made while a sync runs are pushed by one more sync. A change made offline is pushed at the next sync that reaches the repository. Values applied by a sync never start another sync. None of this happens without an account. | 84.5 | AD-325 |
| FR-721 | A drive or bot provider the person uses on another device, and not on this one, is offered under *From your account* in Settings › Sync or Settings › Bots. Nothing is added by itself. *Add…* opens the existing form prefilled, and the person chooses the folder or gives the key. A provider that uses the account is added in one step when the account is usable. | 84.4, 84.5, 84.6 | AD-323, UX-DR118 |
| FR-722 | A Matrix account the person uses on another device is offered above the login tabs. Choosing it prefills the homeserver and the username, or selects the Beeper tab. Settings › Account says that the settings sync with the account, when they last synced, and how many drives, bot providers and Matrix accounts are on the person's other devices. | 84.6 | AD-323, UX-DR118 |
| NFR-97 | **No secret ever enters the repository.** No token, password, session, keychain value or credential value is written to any file keeper writes in the config repository, or committed there. A drive or a bot provider records only the *choice* of credential (`account`, `own` or `none`). No record type has a field that could hold a secret. | 84.4, 84.5 | AD-323, AD-324 |
| NFR-98 | **A synced value never overrides a pin, and never makes a control read-only.** Every layer file still wins over a synced value on read. A synced key without a pin stays editable in the app, and an edit there is a local change. | 84.1, 84.5, 84.6 | AD-320, AD-325 |

**Held, not restated:** NFR-92 (no account, no change): without a configured account the observer does nothing, no file is read, and no block renders. NFR-93 and NFR-95 (secrets and crate boundaries) hold unchanged.

## Stories

Every story names its rung in the three-rung stack (*Stack*, below).
- **The shell is by inspection.** Everything under `src-tauri/crates/keeper/**` awaits CI's macOS job, because the shell crate does not build on this host.
- **Generated bindings** (`src/lib/ipc/gen/*.ts`) are regenerated with `cargo test -p keeper-core` and never hand-edited.
- **Every core and sync test named below is mutation-proved:** mutate, run, restore, and verify the restore by diff.

### 84.1 — Your settings are two files, merged key by key
**Intent:** "is all the settings will be reflected in the file on the sync?" Every preference that can travel is in a file in the repository, and both directions of a change survive. **Rung:** **epic84-core** (lanes CoreSync and SyncRepo). AD-320, AD-324.
**Files:**
- `keeper-core/src/org_account/settings_sync.rs` (new): `Scope`-based `synced_file`, parse and render, `merge`, `Merged`, `FirstSync`.
- `keeper-core/src/org_account/mod.rs`: the `pub mod` line.
- `keeper-core/src/config/keys.rs`: `Shape::to_toml` only.
- `keeper-core/src/registry.rs`: getters and setters for `account.<id>.settings_base.shared` and `.device`.
- `keeper-core/src/org_account/layout.rs`: `is_rewritable`.
- `keeper-sync/src/config_repo.rs`: `Write.replace`; `keeper-sync/tests/config_repo.rs`.

**Acceptance:**
- *Classification* (mutation-proved):
  - `recording.codec`, `notes.embedding_model` and `bots.voice_target` are `Shared`;
  - `hotkey.global`, `notes.active_vault` and `recording.destination_dir` are `Device`;
  - `sdk_encryption` and `sync.git_path` are `None`, and so are a `SessionState` key (`notes.hide_service_files`), a family member (`notes.read.x`, `account.acme.settings_base.shared`) and an unknown key.

  Mutation: dropping the `sync.git_path` exclusion turns it red.
- *Values*: `Shape::to_toml` followed by `Shape::coerce` is the identity for every shape.
- *Files*:
  - render, then parse, is the identity;
  - rendering is deterministic, with sorted keys and the two-line header;
  - an unknown key and a key of the other file survive a render verbatim and are never in `apply`.
- *Merge* (mutation-proved):
  - a first sync with `RemoteWins` pulls a differing remote value;
  - `LocalWins` keeps the local value;
  - a later sync applies a remote change, pushes a local change, and keeps the local value in a conflict;
  - a deletion travels both ways;
  - an unresolved value is kept in `file` and left out of `apply`;
  - A2', a device that lacks the bot: over two syncs `bots.voice_target` stays in `file`, nothing is pushed, and the base holds l;
  - A2', the flip-flop case: B holds Y and A holds X, and neither resolves on the other. After both devices have synced twice, no further change is pushed;
  - A2', a reference that resolves later: once the bot exists, the next sync applies r;
  - `base_if_not_pushed` leaves a locally changed key at b, so the next sync pushes it again.
- *Rewritable* (mutation-proved): `is_rewritable` accepts exactly the five names directly under `<login>/`. It refuses another person's directory, `_template/`, `keeper.toml`, `user.toml`, `devices/*`, `settings.<not a slug>.toml` and a nested `<login>/x/settings.toml`.
- *Transport* (SyncRepo, against a local bare repository, mutation-proved):
  - a `replace` write overwrites an existing file and pushes;
  - a create write still refuses an existing file;
  - `replace` still refuses a symbolic link.

**binds:** FR-713, FR-714, FR-715, FR-719, NFR-98, AD-320, AD-324

### 84.2 — A drive or a bot travels by name, not by number
**Intent:** the vault, ledger, recording destination, voice target and embedding model follow the person, although every id behind them is minted per device. **Rung:** **epic84-core** (lane CoreSync). AD-321.
**Files:** `keeper-core/src/org_account/settings_sync.rs` (`normalize_remote`, `Catalog`, `Catalog::with_bots`, `to_portable`, `from_portable`), and the bots' `normalize_base_url` export if one is needed.

**Acceptance:**
- *Normalisation*:
  - `https://Git.Acme.dev/people/notes.git/` and `https://git.acme.dev/people/notes` give one reference;
  - `https://ana:pw@git.acme.dev/x.git` loses its userinfo;
  - `git@git.acme.dev:people/notes.git` becomes `git@git.acme.dev:people/notes`, otherwise unchanged.
- *Translation both ways* (mutation-proved), for `notes.active_vault`, `tasks.ledger_vault`, `recording.destination_profile_id`, `bots.voice_target`, and the provider inside `notes.embedding_model`, whose `model` passes through untouched:
  - an unknown local id gives `None`;
  - an unresolved reference gives `None`;
  - every other key passes through unchanged.
- *Paths* (A1): a `recording.destination_dir` naming an existing directory is applied, and one naming a missing directory is not.

**binds:** FR-716, AD-321

### 84.3 — A new device starts from your latest one
**Intent:** "if settings exists pull it if not take from other device of the user, if this is the first device create from template." **Rung:** **epic84-core** (lanes CoreSync and SyncRepo). AD-322, AD-324's `last_change_secs`, A3.
**Files:** `settings_sync.rs` (`seed_device`, `seed_shared`); `layout.rs` (`plan_rename` moves `settings.<from>.toml`, A3); `keeper-sync/src/config_repo.rs` (`last_change_secs`) and its test; `keeper-sync/src/lib.rs` (the export).

**Acceptance:**
- *Seeding* (mutation-proved):
  - the most recently changed same-class device wins over an older same-class device, and over a newer device of another class;
  - a `None` time is oldest, and ties go by slug;
  - the chosen seed is `RemoteWins`;
  - with no same-class file, `_template/settings/<class>.toml` seeds with `LocalWins`;
  - with neither, there is no seed;
  - `seed_shared` reads `_template/settings.toml` with `LocalWins`, or gives no seed.
- *History*: `last_change_secs` returns the time of the newer of two commits that touched the path.
- *Rename* (A3): `plan_rename` moves `settings.<from>.toml` when it exists, refuses a destination that exists, and plans nothing extra when the file is absent.

**binds:** FR-717, AD-322, AD-324

### 84.4 — Drives, bot providers and Matrix accounts travel as offers
**Intent:** "Carry all of it." The repository knows what the person uses and on which devices, without a secret and without adding anything by itself. **Rung:** **epic84-core** (lane CoreSync). AD-323.
**Files:**
- `keeper-core/src/org_account/manifest.rs` (new): the four records, parse and render, `merge_drives`, `merge_providers`, `merge_matrix` and `offers`.
- `org_account/mod.rs`: the `pub mod` line.
- `org_account/state.rs`: `AccountOffersVm`, `DriveOfferVm`, `ProviderOfferVm`, `MatrixOfferVm`, `AccountFacts.offers` and `AccountVm.offers`.
- The regenerated bindings, and every `OrgAccountVm` literal in TypeScript (the fixture ripple).

**Acceptance:**
- *Merge* (mutation-proved):
  - an entry this device has gains the device in `devices`;
  - a remote entry this device lacks loses it;
  - an entry whose `devices` becomes empty is dropped;
  - fields come from this device (last writer);
  - an unknown field (`extra`) survives render and parse;
  - the output is sorted by identity.
- *Offers* (mutation-proved): an identity present locally is not offered, including under another spelling of its URL (`https://Git.Acme.dev/x.git` against `https://git.acme.dev/x`).
- *No secret* (NFR-97): no record type has a token, password, session or secret field, and `credential` holds only `account`, `own` or `none`.
- *Bindings*: `bindings:check` is green, and `bunx tsc --noEmit -p .` is green with the fixture ripple on this rung alone.

**binds:** FR-718, FR-721, NFR-97, AD-323

### 84.5 — A change here reaches the repository by itself
**Intent:** "Synced and editable": an edit in the app writes back and pushes, a sync applies what other devices changed, and a pin still wins. **Rung:** the registry observer is on **epic84-core** (lane CoreSync). The shell is on **epic84-surface** (lane Shell). **The shell is by inspection, awaiting CI's macOS job.** AD-325, AD-320's apply, A2', A3.
**Files:**
- `keeper-core/src/registry.rs`: `set_setting_observer` and `with_observer_suppressed`.
- `src-tauri/crates/keeper/src/**`: the boot observer, `note_local_change`, the dirty flag in `sync`, the catalog, the settings and manifest plan, base bookkeeping, the suppressed apply, offers publication, `account_offer_add_provider`, the `note_local_change` call sites, the `resolves` closure, `forget_local_state` clearing both bases, and `account_rename_device` keeping the device base.

**Acceptance:**
- *keeper-core* (mutation-proved):
  - the observer receives the key after a set and after a delete;
  - it receives nothing inside `with_observer_suppressed`, including when suppression is nested;
  - a second `set_setting_observer` call is ignored;
  - the coverage test in `keys.rs` stays green.
- *Shell, by inspection:*
  - the observer calls `note_local_change` only for synced keys, and `note_local_change` does nothing without a configured account;
  - a dirty sync bypasses the throttle; a failed `try_lock` leaves the flag set; a converge that ends dirty runs exactly one more sync;
  - every call site the contract names calls `note_local_change` after its success;
  - the plan re-reads the worktree on every attempt, and every write passes `is_rewritable`;
  - `local` is built from table rows, not from layer-resolved values;
  - the base follows the push outcome;
  - `apply` runs under suppression, through the live setters the contract names;
  - offline, `NotMine` and `Blocked` merge nothing;
  - `account_offer_add_provider` is registered on every target and refuses unless the offer's credential is `account` and the account is usable;
  - *Forget this account* clears both base keys, and a rename keeps the device base.
- *On hesperia and a second device (owed):*
  - `recording.codec` changed on one device appears on the other after its sync;
  - the same key pinned in `<login>/keeper.toml` still wins, and the pane shows the badge;
  - a new desktop starts from the latest desktop's hotkeys (at its next launch);
  - a change made offline is pushed at the next sync;
  - `settings.toml` in the forge holds no secret.

**binds:** FR-715, FR-720, FR-721, NFR-97, NFR-98, AD-320, AD-325

### 84.6 — "From your account"
**Intent:** the other devices' drives, bots and Matrix accounts are one prefilled step away, where each is normally added. **Rung:** **epic84-surface** (lane Front). UX-DR118.
**Files:** the Sync section's drive list, `src/components/settings/bots-section.tsx`, `src/components/auth/login-screen.tsx`, `src/components/settings/account-section.tsx`, the add-folder form's prefill entry, `src/lib/ipc/client.ts` (`accountOfferAddProvider`), and `dev/mock-shell.ts`.

**Acceptance:**
- each block is absent when its list is empty, and the Account line's second sentence is absent without offers;
- the drive offer's `Add…` opens the form prefilled with the name, URL and roles, and preselects the account credential when it is `account` and the account is usable;
- the provider offer's `Add` calls `accountOfferAddProvider(key)`, and a refusal shows inline;
- a Matrix offer prefills the homeserver and the username, and a Beeper offer selects the Beeper tab;
- the Account line's counts and plurals are right;
- *Before done (owed):* the three blocks and the Account line are seen in a real browser against `?account=ready`, by the house method for boot-gated UI (`spec-skipping-setup-can-stick.md:123-125`).

**binds:** FR-721, FR-722, NFR-98, UX-DR118

## What stays out

- **Adding an offered drive, provider or account without the person.** Each needs a folder, a key or a sign-in (AD-323).
- **Pins travelling as settings.** `keeper.toml` and `keeper.<device>.toml` stay the lock, and keeper never rewrites them (AD-324).
- **keeper-syncd.** It keeps its own profiles from its own TOML config (`keeper-syncd/src/commands.rs:1253-1289`), and it neither reads nor writes the settings files.
- **Another person's directory** (D-25, held).

Deferred, with the ledger entries allocated here so a later planner finds them:

```markdown
### DW-300: A conflict is decided per key, not per field inside a JSON value.

origin: epic 84's plan, 2026-09-24 (AD-320, D-26)
location: `src-tauri/crates/keeper-core/src/org_account/settings_sync.rs` (`merge`)
reason: two keys hold JSON: `notes.embedding_model` (`{provider, model}`) and `notes.service_file_names`. When two devices change different parts of one of them between syncs, the device that syncs later wins the whole value, and the other device's part is lost. It is the same rule as for any other key (D-26), applied to a value that is really several. A field-level merge would need a per-key JSON schema and a base for each field. Revisit when a JSON-valued key gains a field two devices plausibly edit independently, or when someone reports a lost half of one.
status: open

### DW-301: A synced key that is read only at launch applies at the next launch.

origin: epic 84's plan, 2026-09-24 (AD-325, FR-715)
location: `src-tauri/crates/keeper/src/hotkey.rs:230`, `:370` (hotkeys registered at launch), the shell's apply step
reason: the four hotkeys (`hotkey.global`, `hotkey.recording`, `hotkey.capture`, `hotkey.voice`) are registered with the OS during setup, and a sync's apply has no live re-registration path for them. A seeded or pulled hotkey therefore sits in the table until the next launch. It is the same "settings read at startup follow at the next launch" rule the account tiers already state (`docs/account.md`, *When a change takes effect*). `debug.mode` and the menu bar go through their live setters and are not affected. Revisit if a person reports a hotkey that "did not sync", or when hotkey registration gains a live apply path the sync can call.
status: open

### DW-302: A drive's identity is its remote and branch, so two profiles of one repository merge into one record and one offer.

origin: epic 84's plan, 2026-09-24 (AD-321, AD-323)
location: `src-tauri/crates/keeper-core/src/org_account/manifest.rs` (`merge_drives`, `offers`), the shell's drive catalog
reason: a device may hold two profiles of the same remote and branch, for example a notes clone and a recordings clone with different subpaths, because nothing prevents it (`sync_ipc.rs:1342-1343`: no duplicate guard). Both map to one identity, so `drives.toml` keeps one record whose fields come from one of them, and another device is offered one drive. A drive reference (`drive:<remote>#<branch>`) likewise resolves to one of the two profiles. Distinguishing them would need a role or subpath in the identity, which would split one drive into several offers for everyone else. Revisit when a person reports two clones of one repository on a device.
status: open

### DW-303: Everyone who can push to the config repository can write every person's directory, so a synced value is exactly as trusted as a pin.

origin: epic 84's plan, 2026-09-24 (AD-324, D-25, D-26)
location: `src-tauri/crates/keeper-core/src/org_account/layout.rs` (`is_own_path`, `is_rewritable`), `docs/account.md` § *Your settings, drives and accounts travel*
reason: keeper's fence limits what keeper writes, never what it reads. In the owner's makistack repository (`keeper/users`), everyone in it can write every directory. Anyone who can push can therefore edit a person's `settings.toml`, and keeper applies it, just as they could already edit that person's `keeper.toml` pins since epic 82. A synced value is less powerful than a pin, since the person can change it back, but it arrives with the same trust. Closing this needs the forge to restrict each directory to its owner (a forge feature, not a keeper one), or keeper to verify that the commits touching `<login>/` are signed by that person, which needs a key that travels safely. Revisit when the repository is shared beyond people who trust each other.
status: open

### DW-304: The only way to act on an offer in the app is to add it.

origin: epic 84's plan, 2026-09-24 (AD-323, UX-DR118)
location: `src-tauri/crates/keeper-core/src/org_account/manifest.rs` (`offers`), the *From your account* blocks in Settings › Sync, Settings › Bots and the login screen
reason: an offer lists everything the person's other devices use and this one does not. A drive the person never wants on this device stays offered until every device that has it removes it, or the entry is deleted from `drives.toml` by hand. A per-device "not here" choice would need a place to live: a device-local setting, or a `declined` list in the record, which every device would have to preserve. Revisit when a person reports an offer they cannot get rid of.
status: open

### DW-305: A Beeper offer only selects the Beeper tab.

origin: epic 84's plan, 2026-09-24 (AD-323, UX-DR118)
location: `src/components/auth/login-screen.tsx` (`BeeperTab`, email → code), `src-tauri/crates/keeper-core/src/org_account/manifest.rs` (`MatrixRecord`)
reason: a Beeper sign-in starts from an email address and a code (`login-screen.tsx:354-357`), and the homeserver is fixed at `matrix.beeper.com`. The record carries the Matrix `user_id`, not the email, so there is nothing to prefill beyond the tab. Carrying the email would put a personal address into the repository for a single field. Revisit if people ask for it.
status: open
```

## The failure shape this epic must not repeat

**A secret in the repository.** Epic 82's rule that no token reaches a file now meets five files keeper rewrites. Every record is a description: a credential is `account`, `own` or `none`, and no record type has a field that could hold more. A review that finds a secret-shaped field in a record, or a value read from the keychain on the way to a `Write`, is a blocker.

**A pin that stops being a pin, or a preference that becomes one.** The settings files are applied into the table, below every layer. A review that finds a synced value installed as a layer, or a `local` built from layer-resolved values (which would push a pin's value as if it were this device's own), is a blocker.

**A loop.** An apply that is not suppressed kicks a sync, whose apply kicks another. The observer's suppression is thread-local, so an apply that hops threads defeats it. The shell's apply must call the setters on the thread that holds the suppression.

## Sprint-status entry

Paste under `development_status:`, above the epic-83 block. The coordinator owns the ledger; this is the text:

```yaml
  # Epic 84: the owner's question on the account build — "is all the settings will be reflected in the file on the sync?": pull them if the file exists, else take them from another of the person's devices, else create them from the template. Owner answered 2026-09-24: synced and editable (new files beside the pins; keeper.toml stays the lock), carry everything (drives, bot providers, Matrix accounts, the preference keys), and a new device's machine-local settings come from the latest device of the same class.
  # Stack rungs: epic84-plan (docs + ledgers), then epic84-core (84.1–84.4 and 84.5's registry observer: settings_sync, manifest, offers VMs, layout::is_rewritable and plan_rename, Shape::to_toml, config_repo Write.replace + last_change_secs, bindings, the OrgAccountVm fixture ripple, and the one-line shell hunks the new fields force), then epic84-surface (84.5's shell sync/apply/command, 84.6's offers UI, mock-shell, docs/account.md). Shell crate by inspection; CI macOS is the gate.
  # Contract amendments A1 (an AbsolutePath value applies only where its directory exists), A2' (superseding A2: a value that cannot be applied here is not applied, and both bases record the local value, so it is never dirty) and A3 (a rename moves settings.<device>.toml and keeps the device base) are in the epic's text.
  # A two-device check on hesperia (a change travels both ways, a pin still wins, a new desktop seeds from the latest desktop, an offline change pushes later, no secret in the forge) is owed, as is 84.6's real-browser proof.
  epic-84: in-progress
  84-1-your-settings-are-two-files-merged-key-by-key: review
  84-2-a-drive-or-a-bot-travels-by-name-not-by-number: review
  84-3-a-new-device-starts-from-your-latest-one: review
  84-4-drives-bots-and-matrix-accounts-travel-as-offers: review
  84-5-a-change-here-reaches-the-repository-by-itself: review
  84-6-from-your-account: review
  # DW-300…DW-305 are opened by this plan; none closes here.

```

## docs/decisions.md entry

Draft for `docs/decisions.md`, to follow D-25 (`docs/decisions.md:1247`). The number is **D-26**, the next free.

```markdown
## D-26 — A synced setting belongs to the device that changed it last

Epic 82 put a person's settings in a git repository as two pin files keeper creates once
and never rewrites. The owner asked for every setting to be in the repository after a
sync, editable in the app and pushed back, and for a new device to start from the
person's others. Epic 84 makes keeper rewrite five files in the person's own directory,
and decides who wins when two devices disagree.

- **What changes:** preferences travel in `<login>/settings.toml` (every device) and
  `<login>/settings.<device>.toml` (this device). Drives, bot providers and Matrix
  accounts travel in `drives.toml`, `bots.toml` and `matrix.toml`, as offers. keeper
  merges each settings file key by key, applies the result into this device's settings
  table (below every layer file, so `keeper.toml` still pins), and pushes. A new device
  seeds its device file from the same-class device whose file changed most recently.
  (AD-320…AD-325; FR-713…FR-722; NFR-97, NFR-98)
- **Who wins:** a device's first sync against an existing file takes the file. After
  that, per key: a key unchanged here takes the repository's value; a key changed only
  here is pushed; a key changed both here and in the repository keeps *this* device's
  value, because the device syncing now is the one whose change reaches the repository
  last. "Last" means last to sync, not last to be edited: a device that was offline for a
  week wins with a week-old edit when it comes back. A value a device cannot apply (a
  drive, a bot or a folder it does not have) is never its change, so it never deletes or
  replaces it.
- **The bounds:** keeper rewrites exactly those five files, directly under the signed-in
  person's own `<login>/`, and never through a link. `user.toml`, `keeper.toml`,
  `keeper.<device>.toml` and `devices/*.toml` stay create-only. No secret travels: a
  drive or a provider records only which credential it uses (`account`, `own` or
  `none`). `sdk_encryption`, `sync.git_path`, every session-state key and every family
  stay on the device. Nothing offered is added by itself.
- **Why this rule:** it needs no clock. Two devices' clocks cannot be trusted to agree,
  and git orders pushes, not edits. Per key rather than per file, because two devices
  changing two different settings between syncs is the common case, and a whole-file
  winner would lose one of them. Per key rather than per field, because only two keys
  hold JSON (DW-300).
- **What it amends:** D-25's refusal to rewrite a file that exists now excepts these
  five files in the person's own directory. Its refusal to write another person's
  directory is unchanged. Epic 82's AD-312 is narrowed the same way (AD-324).
- **What it is not:** a pin. A synced value is a preference the pane can change, and a
  layer file still wins over it. Nor is it a trust boundary: anyone who can push to the
  repository can edit a person's settings, as they already could edit their pins (DW-303).
- **Revisit triggers:** a report of a lost half of a JSON value (DW-300); a repository
  shared beyond people who trust each other (DW-303); a second account per install
  (DW-297), which would need its own answer to whose settings win.
- **Status / owner:** decided. Owner is the architect. Epic 84 implements it:
  `org_account::settings_sync::merge`, `layout::is_rewritable`, and
  `keeper_sync::config_repo`'s `Write.replace`.
```

## Stack

Three rungs, by layer as in epics 80–83:
1. **`epic84-plan`:** this document and the ledgers (sprint-status, `deferred-work.md` DW-300…DW-305, D-26 in `docs/decisions.md`).
2. **`epic84-core`:** keeper-core `settings_sync`, `manifest`, the offers VMs, `layout::is_rewritable` and `plan_rename` (A3), `Shape::to_toml`, the registry observer and the two base keys; keeper-sync `Write.replace` and `last_change_secs`; the regenerated bindings; and every TypeScript `OrgAccountVm` literal that must name `offers`.
   - **Two shell hunks ride this rung**, because the new fields break existing shell literals: `replace: false` at the registration plan's `Write` (`account_ipc.rs:1043-1046`), and `offers` in `facts()` (`account_ipc.rs:389-414`, a literal with no `..Default`). Without them the rung does not compile alone on CI's macOS job. The coordinator moves them at stack time.
   - `bindings:check` must be green on this rung alone.
3. **`epic84-surface`:** the shell (the observer install, `note_local_change`, the sync sequence, the apply, `account_offer_add_provider`, the call sites, the rename's base, forget's bases), the frontend blocks and prefills, `client.ts`, `dev/mock-shell.ts`, and `docs/account.md`.

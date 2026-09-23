# Epic 82 proposal: an optional account, and a per-person config repository

status: approved by the owner 2026-09-23 (precedence amended: account tiers above `~/.keeper`)
source: the owner's request of 2026-09-23 (Polish, verbatim below) and the task
"optional account sign-in (OIDC) + per-person config synced from a git repository".
evidence: repo grounding and external research digests (`local://acct-*.md`,
copied to `research-account-2026-09-23.md` when the epic is cut). Every `path:line`
below was read on 2026-09-23 at `6fbc2c3`.

## 0. The owner's ask

> logowanie za pomoca oauth i uzywanie konfiguracji (sciaganie lub tworzenie) na
> remote - git jak drive ale dostep bedzie juz za pierwszym uruchomieniem. Miej na
> uwadze zeby UI i UX byly spojne i dobre do uzycia - dodaj zarzadzanie
> uzytkownikami - moze onboarding nowych uzytkownikow przez qrcode (wygenerowanie
> przez keepera podczas onboardingu)
> dostep do konta auth ma pozwalac sync konfiguracji i latwiejsze dodawanie innych
> drivow oraz laczenie do botow itp (jezli jest to sync po oauth) - jak nie ma
> internetu to wszystko chodzi normalnie to co jest offline.

In English: sign in with OAuth and use a configuration that lives on a remote.
The remote is git, like a drive, but reachable from the very first launch. UI and UX
must be consistent and easy to use. Add user management, possibly onboarding new
people through a QR code that keeper generates. The account should sync the
configuration, make adding more drives easier, and connect to bots and so on.
Without internet, everything that works offline keeps working.

## 1. What already exists, and what does not

| Need | State | Evidence |
|---|---|---|
| A generic OIDC client (PKCE/state/nonce/id_token) | **absent**. The one OIDC flow is Matrix's, and matrix-sdk owns its PKCE verifier internally. | `keeper-core/src/auth.rs:64-81` (`AuthProvider::authenticate(&Client, …)`), matrix-sdk `auth_code_builder.rs:135-160` |
| Callback routing by `state` | **present, reusable**. It is free of Tauri and Matrix code. | `keeper-core/src/oauth.rs:76-247` |
| A deep-link handler | **present, desktop only**. There is one handler: voice links first, then everything else goes to the OAuth registry. It does not handle a link that launches the app cold, and iOS registers no URL scheme. | `crates/keeper/src/voice_reach.rs:101-113`, `tauri.conf.json:52-55`, `gen/apple/**` (no `CFBundleURLTypes`) |
| ASWebAuthenticationSession / Custom Tabs | **absent**. Sign-in opens the system browser through `tauri_plugin_opener`. | `crates/keeper/src/ipc.rs:709-714, 894-898` |
| Keychain port with a cache that avoids repeated ACL prompts | **present** | `keeper-core/src/platform.rs:26-84, 104-224` |
| reqwest + rustls HTTP client | **present**. No new TLS stack is needed. | `src-tauri/Cargo.toml:84-85` |
| Git clone/fetch with a credential supplied per request | **present**. It uses gix and never a credential helper. Basic auth only today. | `keeper-sync/src/git/repo.rs:587`, `git/fetch.rs:130, 219-241` |
| Git push without the git binary | **present** (the phone's smart-HTTP push, reqwest) | `keeper-sync/src/git/push_http.rs:128` |
| Layer files with fault reporting and the "Set by a file" badge | **present, frozen at boot** (`OnceLock`) | `keeper-core/src/config/mod.rs:95-166, 459-637, 770-847`; `src/components/settings/config-source-section.tsx:87-102` |
| Per-device identity | **half-present**. There is a sync.db device label, but no device class and no hostname on iOS (`hostname` does not exist in the sandbox). | `keeper-sync/src/db.rs:1626-1699`, `config/mod.rs:333-345` |
| QR rendering | **present**, in Rust as SVG. keeper has **no** QR scanner. | `keeper-core/src/bridges/login.rs:37-64` |
| Forge API base from the remote URL | **broken** for subpath forges: `https://host/git/owner/repo.git` → `None` | `keeper-sync/src/engine.rs:36685-36736` |
| Android | **absent** (no target, no `Platform`) | `crates/keeper/src/ipc.rs:460-464` |

## 2. The configuration model

### 2.1 Two different things, two different stores

**The account descriptor** answers *who am I and where do my settings live*. It
is kept in its own store, **not** in the layer files. Reasons:

1. **It must be read before the layer stack can resolve.** The account tiers
   (§2.4) are found *through* it. A descriptor inside a layer file would be read by
   the stack it feeds.
2. **The config repository must not be able to redirect itself.** If `[account]`
   were a layer table, `<login>/keeper.toml` inside the repo could name a different
   issuer or repo. A descriptor outside the stack closes that loop.
3. **The layer parser deliberately refuses unknown tables.** It tells the user
   where a key belongs (`config/mod.rs:620-631`). Opening it to a table that only
   one file may carry would be a new per-tier exception in a parser that today has
   none (all tier policy lives in `LayerTier`'s predicates, `mod.rs:118-166`).

Where it lives:
- **desktop:** `~/.keeper/account.toml`, in the directory the person already
  edits by hand;
- **iOS:** `<app data dir>/account.toml`. keeper writes it; no file system access
  is needed.

It is **one file with one schema**. The operator serves it as JSON (§2.3); keeper
stores it as TOML, the same serde struct in both. It gets its own fault list,
reported in Settings in the same place and with the same wording as layer faults
(a new `ConfigTierVm::Account`). A malformed or missing required field means
**no account, with the reason shown**. Nothing else changes.

**One account per install**, in this epic. The config repo defines who is using
keeper. Two people on one device is two OS users. The `id` is in the schema, the
redirect URI and the keychain keys, so a list can come later without a migration.

### 2.2 Schema (the TOML form)

```toml
version = 1
id = "acme"                       # [a-z0-9-]{1,32}; keychain keys, redirect URI
name = "Acme"                     # shown in Settings and on the sign-in sheet

[auth]                            # the one identity (OIDC)
issuer = "https://id.acme.dev"    # exact string; discovery at <issuer>/.well-known/openid-configuration
client_id = "keeper"              # public native client; a secret is refused at parse time
scopes = ["openid", "profile", "email", "offline_access"]      # the default
extra_scopes = []                 # provider-specific: audience, roles
trusted_audiences = []            # extra `aud` values the id_token may carry (Zitadel adds its project id)
redirect_uri = "keeper://oauth/acme/callback"   # default: keeper://oauth/<id>/callback
                                  # desktop may say "http://127.0.0.1/callback" (ephemeral port) or pin a port
username_claim = "preferred_username"           # default
roles_claim = "groups"            # optional: array of strings OR object whose keys are roles
required_role = "keeper"          # optional: sign-in without it is refused with a sentence

[auth.endpoints]                  # optional overrides of discovery, each optional
# authorization = "…"; token = "…"; userinfo = "…"; revocation = "…"; end_session = "…"; jwks = "…"

[config]                          # the config repository (git over HTTPS)
url = "https://git.acme.dev/git/people/keeper-config.git"
branch = "main"                   # default
api_base = "https://git.acme.dev/git/api/v1"   # optional; ALWAYS explicit, never derived from `url`
identity_field = "sub"            # which user.toml field holds the sign-in `sub`; e.g. "zitadel_id"

[config.auth]
mode = "same"                     # default. same | oauth | none

# mode = "same": the sign-in access token is the git HTTP credential
scheme = "basic"                  # basic (default) | bearer
username = "oauth2"               # basic only; the token is the password

# mode = "oauth": a forge that only accepts its own tokens (Gitea/Forgejo-style)
# issuer = "https://git.acme.dev/git"            # OR authorize_url + token_url
# client_id = "…"
# scope = "openid profile read:user write:repository"   # ONE string, sent byte-identical forever
# redirect_uri = "keeper://oauth/acme/forge/callback"   # the default
# signin_url = "https://git.acme.dev/git/user/oauth2/acme-id?redirect_to={authorize_path_and_query}"
# user_url = "{api_base}/user"    # default; used when the forge's id_token carries no username
# username_field = "login"        # default
```

Defaults and refusals are enforced by the parser and pinned by tests:
- `config.auth.mode = "same"` and `scheme = "basic"`, `username = "oauth2"`;
- `client_secret` anywhere → refused ("keeper is a public client; remove the
  secret");
- non-`https` `issuer` / `url` / `api_base` → refused, except loopback hosts, which
  are accepted for test providers;
- `mode = "oauth"` without a way to learn the forge username (a forge `issuer`
  with `openid` in `scope`, or `api_base`) → refused, because the username check
  (§3.5) cannot run;
- `api_base` is never filled in from `url`. There is no code path that derives it.
  `forge_api_target` is **not** called for the config repo.

### 2.3 Bootstrap: one input

The operator serves the descriptor as JSON at any HTTPS URL. The recommended URL is
`https://<issuer-host>/.well-known/keeper-account.json`. A device receives it in
one of three ways:

1. **A link:** `keeper://setup?descriptor=<url-encoded https URL>`. This works from
   a web page, a chat message, or the iOS Camera app reading a QR code.
2. **An inline link, for operators without hosting:**
   `keeper://setup?d=<base64url(JSON)>`. A typical descriptor is about 600 bytes,
   well within a QR code.
3. **Pasting either link, or a bare `https://…json` URL,** into the field on
   keeper's sign-in screen. This covers a Mac, which cannot scan.

Whatever the input, keeper fetches the descriptor (HTTPS only, no redirects,
64 KB cap) and parses it. It then shows a **confirmation sheet**: the account
name, the sign-in host, the repository host, and "Continue" / "Cancel". Nothing is
written before the person continues. A setup link can point keeper at anyone's
identity provider, so the hosts are always on screen.

**Onboarding by QR code.** On a device that is signed in, Settings › Account ›
*Add a device or a person* shows the setup link as a QR code (rendered by the
existing `qr_svg`), plus a "Copy link" button. The descriptor is the same for
everyone. A new person scans it, signs in as themselves, and keeper creates their
directory from the template (§3.3). **That is the whole of "user management"
keeper does.** People, roles and access belong to the identity provider and the
forge. keeper shows who you are, your roles and your devices, and it never edits
another person's directory. An admin surface over other people's directories is
proposed as deferred work: the repo may let everyone write everywhere, and keeper
must not be the tool that does it.

**Desktop hand-editing:** write `~/.keeper/account.toml`. It is read at launch and
whenever Settings opens.

### 2.4 Where the account's layer files sit in the stack

The current stack (later wins, per key; `config/mod.rs:12-24`):

```text
~/.keeper/keeper.toml                  user, every machine
~/.keeper/keeper.<host>.toml           user, THIS machine
<clone>/<login>/keeper.toml            account, every device       (AccountShared)   ← NEW
<clone>/<login>/keeper.<device>.toml   account, this device        (AccountDevice)   ← NEW
<main>/.keeper/keeper.toml             the main sync folder, every machine
<main>/.keeper/keeper.<host>.toml      the main sync folder, THIS machine
<folder>/.keeper/keeper.toml           that folder only
<folder>/.keeper/keeper.<host>.toml    that folder, this machine
```

**Decided (owner, 2026-09-23): the account tiers sit above `~/.keeper` and below
the main folder.** They behave like `~/.keeper/keeper.toml` and
`keeper.<host>.toml` (same parser, same `[settings]` rights, same machine-local key
rule for the device file), and the repository wins over this machine's hand edits
in `~/.keeper`. The main sync folder and per-folder files keep their current
authority. *(The proposal had put them lowest; the owner chose the repo to win.)*

- **`mainSyncFolder`** in an account file is refused, with the existing
  "only `~/.keeper/` may elect the main folder" fault (AD-101).
- **`[folder]`** in an account file is refused with the existing
  `has_folder == false` fault.
- **Unknown keys and wrong shapes** produce the same per-key faults as today. The
  rest of the file still applies.
- **Live, not frozen.** The account tiers are held in their own `RwLock`, consulted
  by `setting_override` below the frozen `OnceLock` stack. A fetch that changes
  this person's files swaps them in. Keys read on every access take effect at once.
  Keys consumed only at boot (hotkeys, the debug log) take effect at the next
  launch. Settings says so, next to the badge.
- **Offline:** at launch the tiers are read from the last clone on disk, so an
  unreachable network gives yesterday's settings, not none.

### 2.5 Example descriptors

**A. A generic OIDC provider (Zitadel) plus a Forgejo forge using `oauth` mode**,
the forge entered through its own login with the same identity provider:

```json
{
  "version": 1,
  "id": "acme",
  "name": "Acme",
  "auth": {
    "issuer": "https://id.acme.dev",
    "client_id": "283746519283746519@keeper",
    "extra_scopes": [
      "urn:zitadel:iam:org:projects:roles",
      "urn:zitadel:iam:org:project:id:283746519283746001:aud"
    ],
    "trusted_audiences": ["283746519283746001"],
    "username_claim": "preferred_username",
    "roles_claim": "urn:zitadel:iam:org:project:283746519283746001:roles",
    "required_role": "keeper"
  },
  "config": {
    "url": "https://git.acme.dev/git/people/keeper-config.git",
    "branch": "main",
    "api_base": "https://git.acme.dev/git/api/v1",
    "identity_field": "zitadel_id",
    "auth": {
      "mode": "oauth",
      "issuer": "https://git.acme.dev/git",
      "client_id": "6f1c2a8e-4b1d-4c55-9e0a-2d7f1b3c9a10",
      "scope": "openid profile read:user write:repository",
      "signin_url": "https://git.acme.dev/git/user/oauth2/acme-id?redirect_to={authorize_path_and_query}"
    }
  }
}
```

**B. The same identity provider, with `same` mode.** The repository sits behind
something that accepts the provider's access token: oauth2-proxy in front of
Gitea/Forgejo, Forgejo 16 "Authorized Integrations", or a test server.

```json
{
  "version": 1,
  "id": "acme",
  "name": "Acme",
  "auth": {
    "issuer": "https://id.acme.dev",
    "client_id": "283746519283746519@keeper",
    "extra_scopes": [
      "urn:zitadel:iam:org:projects:roles",
      "urn:zitadel:iam:org:project:id:283746519283746001:aud"
    ],
    "trusted_audiences": ["283746519283746001"],
    "roles_claim": "urn:zitadel:iam:org:project:283746519283746001:roles",
    "required_role": "keeper"
  },
  "config": {
    "url": "https://git.acme.dev/git/people/keeper-config.git",
    "identity_field": "zitadel_id",
    "auth": { "mode": "same", "scheme": "basic", "username": "oauth2" }
  }
}
```

Omitted fields take their defaults. In B, `config.auth` could be omitted entirely:
`same`, basic, `oauth2`.

## 3. Behaviour

### 3.1 Sign-in

- **The browser.** ASWebAuthenticationSession on macOS and iOS, with
  `prefersEphemeralWebBrowserSession = false`. It is called from Rust through
  `objc2-authentication-services`, one module shared by both Apple targets, under
  audited `#[allow(unsafe_code)]` entries in the inventory. Android is Custom Tabs /
  Auth Tab when an Android target exists; there is none today, so it is recorded
  as deferred work, not a stub.
  - It shares the identity provider's cookies with the default browser, so the
    forge's `oauth` leg (and its `signin_url`) usually needs no second password.
    What remains is the system "keeper wants to sign in" alert and possibly the
    forge's consent page. Forgejo shows consent on every authorize request for a
    public client.
  - This is also where 1Password and iCloud passkeys are offered. keeper needs no
    passkey code, because it never uses an embedded webview.
- **Loopback on desktop.** A loopback `redirect_uri` uses an RFC 8252 listener on
  `127.0.0.1` and the default browser instead of the sheet.
- **Redirect.** Default `keeper://oauth/<id>/callback`. The forge leg uses
  `keeper://oauth/<id>/forge/callback`. Both are distinct from Matrix's
  `keeper://oauth/callback` (`oauth.rs:31`). RFC 8252 §7.1 recommends a reverse-DNS
  scheme (`dev.tgorka.keeper:/oauth/…`). The default stays `keeper://` as asked,
  and any value may be configured.
- **Wiring.** The shell's completion handler hands the callback URL to the
  existing `OAuthFlowRegistry`, keyed by `state`, exactly as a deep link does today.
  Cancel resolves as `Cancelled`. `Platform` gains one port method,
  `start_web_auth(url, callback)`.
- **iOS prerequisites.** `CFBundleURLTypes` for `keeper` (for `keeper://setup`),
  the `AuthenticationServices` framework link, and handling of a link that launches
  the app cold (`deep_link().get_current()`).
- **Validation, in keeper-core with `openidconnect` 4 (MIT).** Built without default
  features, on the existing reqwest 0.13 through `oauth2-reqwest`, which is already
  in the lock via matrix-sdk. keeper checks:
  - fresh `state`, `nonce` and PKCE S256 per attempt, each consumed exactly once;
  - `iss` equal to the issuer byte for byte;
  - `aud` equal to `client_id` plus `trusted_audiences` only;
  - `azp`;
  - an allowed-algorithm set;
  - `exp`/`iat` with 60 s leeway;
  - `at_hash` when present;
  - one JWKS refetch on an unknown `kid`;
  - UserInfo `sub` equal to the id_token `sub`.
- **Claims.**
  - The username comes from `username_claim`. It must be a safe directory name:
    `[A-Za-z0-9._-]`, no leading `.` or `_`, no `/`. Otherwise sign-in is refused
    with a sentence.
  - Roles come from `roles_claim`, from the verified id_token or else from
    UserInfo, **never from the access token**. The claim may be an array of strings,
    or an object whose keys are the roles. Nested paths (`realm_access.roles`) are
    accepted when no top-level claim has that exact name.
  - If `required_role` is set and missing, sign-in is refused with
    "Your Acme account does not have the keeper role. Ask your administrator."

### 3.2 Tokens: a general credential

- **Storage.** Tokens live only in the keychain, as **one** item
  `account/<id>/session` (refresh, access, expiry, id_token, iss, sub), so there is
  one ACL prompt on macOS, not three. The forge leg has its own item,
  `account/<id>/forge`. On iOS items are `AfterFirstUnlockThisDeviceOnly`, never
  synchronizable. A refresh token is per device.
- **One internal API:** `account::access_token(account_id) -> Result<AccessToken>`.
  - It returns a valid token, refreshing 60 s early under a per-account async
    mutex. That makes the app process the single refresher: rotation is universal,
    so two refreshers lose the session.
  - A rotated refresh token is written to the keychain **before** the new access
    token is used.
  - Errors are `NeedsSignIn` (grant dead) or `Unreachable` (network). They are
    distinct, because the UI says different things.
  - Git is one consumer of this API, not its owner.
- **Sign-out.** Revoke the refresh token(s) if a revocation endpoint exists (RFC
  7009). Open the end-session endpoint with `id_token_hint` in the same browser
  session. Delete the keychain items. **Never delete the clone's files**; the clone
  is kept as the "last synced config", and its tiers stop applying.

### 3.3 Config repository contract

This is the owner's layout, unchanged. `_`-prefixed directories are not people.

**Resolve.**
1. Look up `<login>/` for the signed-in username.
2. Read `user.toml`.
3. Require its identity field, `identity_field` (default `sub`), to equal the
   token's `sub`.
4. If `user.toml` also records `issuer`, it must match too.
5. On a mismatch, **stop**: no account tiers load, and a blocking status sentence
   names the directory and says the sign-in belongs to someone else. keeper never
   loads another person's directory.

**`user.toml`** as keeper writes it:

```toml
login = "tgorka"
display_name = "Tomasz Gorka"
sub = "283746519283746777"     # the field named by identity_field
issuer = "https://id.acme.dev"
created = "2026-09-23T10:12:00Z"
```

A repo whose `user.toml` records `zitadel_id` instead of `sub` works with
`identity_field = "zitadel_id"`. keeper never rewrites an existing `user.toml` to
migrate it.

**Create, first sign-in, no `<login>/`.** Copy `_template/keeper.toml` →
`<login>/keeper.toml`, write `user.toml`, commit, push.

**New device.** Write `<login>/devices/<device>.toml`
(`name`, `class`, `platform`, `created`). Copy `_template/class/<class>.toml` →
`<login>/keeper.<device>.toml` when the template exists. Commit, push.

**Rules.**
- A file that already exists is never re-copied or rewritten.
- Only paths under `<login>/` are ever staged. The staging function refuses any
  other path, and a test pins it.
- A push rejected as non-fast-forward (someone else pushed) triggers a fetch, a
  re-apply of the create-only writes onto the new tip (skipping files that now
  exist), and a retry, up to 3 times.
- A second launch finds everything present and writes nothing.

**Transport.**
- Clone and fetch go through gix with the per-request credential callback, as
  drives do: `same`/basic is the token as password with `username`; `bearer` is
  `http.extraHeader` as an in-memory override.
- Push goes through `push_http` on every platform (reqwest; no git binary, no token
  in argv), with the same credential spelling.
- The clone lives in the app data dir (`<data>/account/<id>/repo`), is not a drive,
  and appears in no drive list.

**Device name and class come from keeper, never from a token.**
- **Name:** the short hostname lower-cased to `[a-z0-9-]`. On iOS, where
  `hostname` does not exist, it is the device model plus a 4-character suffix
  (`iphone-3f2a`).
  - It is editable on the sign-in sheet before the device is first registered.
  - A later rename moves both of this device's files inside `<login>/` in one
    commit.
- **Class:** macOS/Linux/Windows → `desktop`. iOS → `tablet` when
  `UIDevice.userInterfaceIdiom == .pad`, else `mobile`. Android: by
  smallest-width ≥ 600dp, when Android exists.

### 3.4 Offline / unreachable

- If sign-in cannot start (issuer unreachable), keeper says so and stays
  local-only.
- Once signed in:
  - an unreachable fetch keeps the last clone's tiers applied;
  - the status reads *Offline — using settings from 14:02*;
  - a retry runs on the next launch, on Settings › Account › *Sync now*, and every
    15 minutes while keeper runs.
- Nothing about an unreachable network deletes, rewrites or disables a local
  setting.
- A dead grant (refresh rejected) is *Sign in again*, which is different from
  offline.

### 3.5 The `oauth` connector

- It is a second PKCE flow against the forge, in the same browser session.
- If `signin_url` is set, keeper opens it with `{authorize_path_and_query}` filled
  with the forge's authorize path and query, so the forge's own identity-provider
  login carries the person through.
- The `scope` string is sent byte-identical on every device and version.
- **Username check.** The forge username comes from the forge's id_token
  (`preferred_username`) or `user_url`/`username_field`. If it differs from the
  sign-in username, the connection is refused and its tokens are discarded:
  *"The forge signed you in as ana, but your keeper account is tgorka."*
- The forge's `401` → refresh once → retry. A second `401` or a `403` →
  *Reconnect the repository*.
- **Deferred work, recorded:** on Forgejo, refresh tokens are invalidated across
  devices by default (`INVALIDATE_REFRESH_TOKENS`).

### 3.6 The account as a credential elsewhere ("easier drives, bots")

- **Drives.** The add-drive form gets a credential choice, *Use my Acme account*.
  The drive then asks `access_token()` per operation instead of storing a pasted
  token. Its keychain key is not created. Username/scheme follow the account's
  `config.auth` in `same` mode.
- **Bots.** A bot provider gets the same choice, sending `Authorization: Bearer`
  with the account token.
- Both are opt-in per drive or provider. Nothing switches over by itself.

## 4. UI and UX (house patterns, reused)

- **Settings › Account.** A new first section, present on every tier.
  - Signed out: one field, *Paste a setup link*, and a line explaining that keeper
    works fully without an account.
  - Signed in:
    - identity row: display name, `login`, issuer host;
    - roles as chips;
    - status sentence (Up to date / Offline since… / Sign in again / Blocked: …);
    - *Sync now*;
    - the device list from `<login>/devices/` (this device marked, *Rename*);
    - *Add a device or a person* (QR sheet);
    - *Sign out…* in an AlertDialog that says the repository files are kept.
- **Setup confirmation sheet.** Account name, sign-in host, repository host, and
  editable device name and class. It is the same sheet whether it was reached by
  link, QR code or paste.
- **First-run wizard.** An optional first step, *Sign in with an organisation
  account*. It appears only when a setup link started keeper or the person picks
  it; skipping keeps today's flow.
- **Status.** A one-line account status sits beside the sync status: offline,
  sign in again, blocked. Nothing modal interrupts work.
- **The "Set by a file" badge** names the repo file (`acme: tgorka/keeper.toml`),
  and the fault list shows account-file faults beside the others.

## 5. Stories (proposed stack)

| # | Story | Rung |
|---|---|---|
| 82.1 | Descriptor: schema, defaults, refusals, `account.toml` store and faults, setup-link parsing, HTTPS fetch, confirmation VM | core |
| 82.2 | OIDC client: discovery, PKCE/state/nonce, id_token checks, claims (username, both role shapes, required role), keychain session, single-flight refresh, `access_token()`, sign-out | core |
| 82.3 | Config repo: clone/fetch/push with the account credential, resolve and `sub` check, create-from-template, device registration, create-only, own-dir-only, offline fallback, `oauth` connector and username check | core |
| 82.4 | Account layer tiers: two new tiers, live swap, faults, phrases, generated docs | core |
| 82.5 | Shell: ASWebAuthenticationSession port (macOS + iOS), loopback, deep-link routes (`keeper://setup`, `keeper://oauth/<id>/…`), cold-start links, iOS URL scheme and framework, device class, IPC | shell |
| 82.6 | UI: Settings › Account, confirmation sheet, wizard step, QR sheet, status line, device rename | surface |
| 82.7 | The account as a credential for drives and bot providers | surface |

**Acceptance.** Acceptance 1–5 of the task, unchanged. Acceptance 2 (fresh macOS
and iOS installs, a 1Password passkey, both modes) is a device check on hesperia
and an iPhone. The epic records it as owed until run.

## 6. Decisions for the owner

1. **Precedence.** *Decided: account tiers above `~/.keeper`, below the main
   folder.* (Proposed was lowest.)
2. **Descriptor store.** A separate `~/.keeper/account.toml` (iOS: app data)
   (proposed), versus a new `[account]` table in `~/.keeper/keeper.toml`.
3. **Redirect default.** `keeper://oauth/<id>/callback` as asked (proposed), versus
   the RFC 8252 reverse-DNS `dev.tgorka.keeper:/oauth/<id>/callback`.
4. **Scope of this epic.** All of §5 including 82.7 (proposed), or stop after 82.6
   and ledger 82.7.
5. **User management.** keeper manages only *your* identity and devices, and
   onboards others via the QR/link (proposed). Admin editing of other people's
   directories is deferred.

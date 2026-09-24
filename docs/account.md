# Account

An optional organisation account for keeper. You sign in once with your organisation's
identity provider (OpenID Connect), and keeper keeps your settings in your own directory of
a shared git repository, on every device you use.

This is the user and operator document. It covers:
- what the account is and is not;
- the descriptor an operator writes;
- how a device is set up;
- what keeper reads and writes in the repository;
- where the account's settings sit among the other layer files;
- how your settings, drives, bot providers and Matrix accounts travel between your devices;
- what happens offline, at sign-out and when you forget the account;
- the security properties it keeps.

The design reasoning lives in `_bmad-output/planning-artifacts/`:
`epic-82-an-optional-account-and-a-config-that-follows-you.md` for the decisions and the
requirement numbers below, `epic-84-your-settings-follow-you.md` for the settings that
travel, and `research-account-2026-09-23.md` for the evidence.

## The one idea

**An account is a descriptor, a sign-in, and a directory.**

- **The descriptor** is a small file that says who signs you in and where your settings live.
  An operator writes it once for everyone. It reaches a device through one input: a link, a
  QR code, or a pasted URL.
- **The sign-in** is your organisation's own login page, in the system's sign-in window.
  keeper never sees your password. The token it gets back is kept in the OS keychain and is
  a general credential: the config repository uses it, and so can a drive or a bot provider
  if you choose.
- **The directory** is `<your login>/` in the config repository. keeper creates it from a
  template on your first sign-in and adds each device you use to it. It applies the pins
  in it as layer files, and keeps your preferences, drives, bot providers and Matrix
  accounts there in step with every device.

**keeper works fully without an account.** With no descriptor nothing changes: no file is
read that can fail, no network request is made, and Settings › Account shows only a paste
field and a sentence saying so.

**What the account is not:**
- **User management for other people.** keeper shows who you are, your roles and your
  devices, and onboards others by showing a QR code of the setup link. People, roles and
  access belong to the identity provider and the forge. keeper never edits another person's
  directory (D-25).
- **One account for two people.** There is one account per install. Two people on one
  device are two OS users.
- **A replacement for the Matrix accounts in the sidebar.** Those are unchanged, and the two
  kinds of account are independent.

## What an operator provides

1. **An OpenID Connect provider** with a *public, native* client for keeper: no client
   secret, PKCE S256, and the redirect URI `keeper://oauth/<id>/callback` registered
   exactly. See *Operator notes* for Zitadel, Keycloak, authentik and Authelia.
2. **A git repository over HTTPS** (the *config repository*) that every person can clone and
   push to, holding a `_template/keeper.toml` for keeper to copy for each new person (see
   *The config repository*).
3. **A credential path from the sign-in to that repository**, in one of three modes (see
   `config.auth` below):
   - `same`: the repository accepts the identity provider's access token;
   - `oauth`: the forge (Gitea or Forgejo) signs the person in again through its own OAuth
     app;
   - `none`: keeper sends no credential, for a repository that needs none (a local test
     server, for example).
4. **The descriptor**, served as JSON at an HTTPS URL, or packed into the link itself.

## The descriptor

keeper stores it as TOML:
- **desktop:** `~/.keeper/account.toml`, beside the layer files you already edit by hand;
- **iOS:** `account.toml` in the app's data directory. keeper writes it there, so no file
  access is needed.

An operator serves the same structure as JSON. Every field has one meaning in both forms.

### Schema

```toml
version = 1
id = "acme"                       # [a-z0-9-]{1,32}; used in keychain keys and the redirect URI
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

### Defaults

| Field | Default |
| --- | --- |
| `auth.scopes` | `["openid", "profile", "email", "offline_access"]` |
| `auth.username_claim` | `preferred_username` |
| `auth.redirect_uri` | `keeper://oauth/<id>/callback` |
| `config.branch` | `main` |
| `config.identity_field` | `sub` |
| `config.auth` | `mode = "same"`, `scheme = "basic"`, `username = "oauth2"` |
| `config.auth.redirect_uri` (oauth) | `keeper://oauth/<id>/forge/callback` |
| `config.auth.user_url` (oauth) | `{api_base}/user` |
| `config.auth.username_field` (oauth) | `login` |

`roles_claim`, `required_role`, `api_base` and every `auth.endpoints` entry have no default:
when absent, keeper does without them.

### What keeper refuses

A descriptor is refused, with a sentence, and **no account** is set up when:
- it carries a client secret anywhere ("keeper is a public client; remove the secret");
- `issuer`, `config.url`, `config.api_base` or an endpoint is not `https`. Loopback hosts are
  the exception, for test providers;
- `id` does not match `[a-z0-9-]{1,32}`;
- `config.auth.mode = "oauth"` gives keeper no way to learn the forge username: neither a
  forge `issuer` with `openid` in `scope`, nor `api_base`. Without it, the username check
  (see *Signing in*) cannot run.

A malformed file, or a refused one, shows its reason in Settings' fault list under
`account.toml`, next to the other layer files' faults. Nothing else changes.

**`api_base` is never filled in from `url`.** A clone URL cannot say where a forge's API
lives: `https://h/a/b/c.git` fits a forge installed at `/a/` as well as one at the root
with a nested group. Give `api_base` when you need it.

### Two example descriptors

**A. A generic OIDC provider (Zitadel) and a Forgejo forge in `oauth` mode.** The forge is
entered through its own login, backed by the same identity provider:

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

**B. The same identity provider, in `same` mode.** The repository sits behind something
that accepts the provider's access token. That can be oauth2-proxy in front of Gitea or
Forgejo, Forgejo 16's *Authorized Integrations*, or a test server:

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

Omitted fields take their defaults. In B, `config.auth` could be left out entirely: `same`,
`basic` and `oauth2` are the defaults.

## Setting up a device

### Serving the descriptor

Serve the JSON at any HTTPS URL. The recommended place is
`https://<issuer-host>/.well-known/keeper-account.json`. keeper fetches it:
- over HTTPS only;
- with **no redirects**: a URL that answers with a redirect is refused, so serve the
  descriptor at its final address;
- reading at most **64 KiB**.

A typical descriptor is about 600 bytes.

### One input: a link, a QR code, or a paste

A device receives the descriptor in one of three ways. All of them lead to the same
confirmation.

1. **A link:** `keeper://setup?descriptor=<url-encoded https URL>`. It works from a web page,
   a chat message, or the iOS Camera app reading a QR code, whether keeper is running or
   the link starts it.
2. **An inline link, for operators without hosting:** `keeper://setup?d=<base64url(JSON)>`.
   The whole descriptor travels in the link.
3. **A paste or a scan.** Paste either link, or a bare `https://…json` URL, into the setup
   field, or choose *Scan a QR code* beside it and hold the code up to the camera. The field
   is in three places, and they behave the same: Settings › Account, the first-run step
   *Sign in with an organisation account*, and **Add account › keeper account…** at the
   foot of the sidebar.

   Scanning runs inside keeper's window. The camera preview and the decoding (zxing-cpp,
   compiled to WebAssembly and shipped inside the app, so nothing is downloaded) stay on
   the device. Only the decoded text leaves the scanner, and it goes to the same place a
   paste goes. The camera stops as soon as a code is read, when you cancel, and when the
   window closes or is hidden (keeper keeps running, the scan does not). macOS asks once for
   camera access, with the same sentence recording uses. If you refused, the scanner names
   the System Settings pane and offers to open it; a camera that stops mid-scan says so.

   *Scan a QR code* appears only where the window can reach a camera: on the Mac, not in
   the iOS app. On an iPhone, point the Camera app at the code, which opens the link in
   keeper.

keeper then shows **one confirmation sheet**:
- the account's name;
- **the sign-in host and the repository host**, in full;
- this device's name, which you can edit until the device is registered, and its class.

**Nothing is written before you choose *Continue*.** *Cancel* writes nothing. A setup link
can point keeper at anyone's identity provider, which is why both hosts are always on the
screen. Continue only for hosts you recognise.

If this device already has a different keeper account, the sheet also says so ("This
replaces *name* on this device …"). *Continue* signs you out of that account and forgets
it here, then sets up the new one. The old account's files in its settings repository are
kept.

**Add account** at the foot of the sidebar offers both kinds of account: *Matrix account…*
opens the Matrix sign-in, as before. *keeper account…* opens the setup field. Once an
account is configured it reads *Change keeper account…* and says that a link for another
account replaces the current one. Whether a link really does is decided when it is read,
and the confirmation sheet says so.

After *Continue*, keeper:
1. writes `account.toml`;
2. signs you in (see *Signing in*);
3. in `oauth` mode, connects the forge;
4. clones or fetches the config repository;
5. creates or registers what is missing in your directory, and brings your settings files
   up to date (see *Your settings, drives and accounts travel*);
6. commits and pushes;
7. applies your settings.

Progress and the outcome appear in the same sheet, as one sentence.

### Adding a device or a person

On a signed-in device, open Settings › Account › *Add a device or a person*. It shows the
setup link as a QR code on a white card, the link itself, and *Copy link*. The descriptor
is the same for everyone. A new person scans it, signs in as themselves, and keeper creates
**their** directory from the template. A new device of yours scans it, and keeper registers
the device in your directory.

The link carries no secret (see *Security notes*). Sharing it grants nothing: access is
decided by the identity provider's sign-in and the forge's permissions.

### Hand-editing on desktop

You can write `~/.keeper/account.toml` yourself instead. keeper reads it at launch and
whenever Settings opens.

## Signing in

**The window.**
- **macOS and iOS:** keeper uses the system's web authentication session
  (ASWebAuthenticationSession), which shares your default browser's sign-in. If you are
  already signed in to your organisation in the browser, you see the system's "keeper wants
  to sign in" alert and little else. Passkeys, iCloud Keychain and 1Password work there as
  they do in the browser. keeper never uses an embedded web view.
- **Desktop, with a loopback redirect** (`redirect_uri = "http://127.0.0.1/callback"`, or a
  pinned port): keeper opens the default browser and listens on `127.0.0.1` for exactly one
  request.
- **Other desktops** (Linux, Windows) open the default browser and receive the callback
  through the `keeper://` link.

*Cancel*, in keeper or in the sign-in window, ends the attempt as cancelled, not failed. An
abandoned sign-in ends by itself after five minutes.

**What keeper checks.** keeper checks every sign-in as a public native client must:
- a fresh `state`, `nonce` and PKCE S256 verifier per attempt, each used once;
- the ID token's issuer, byte for byte;
- its audience: your `client_id`, plus only the `trusted_audiences` you listed;
- `azp`, the signing algorithm, expiry and issue time (60 s leeway), and `at_hash`;
- UserInfo, only when its `sub` matches the ID token's.

A callback whose `state` or `nonce` does not match is refused.

**Who you are.**
- **Your login** comes from `username_claim`. It must be a safe directory name:
  `[A-Za-z0-9._-]`, not starting with `.` or `_`, no `/`, at most 64 characters. Otherwise
  sign-in is refused with a sentence.
- **Your roles** come from `roles_claim`, in the verified ID token or else in UserInfo. They
  are **never read from the access token**. The claim may be a list of strings (`["keeper",
  "ops"]`) or an object whose keys are the roles (Zitadel's shape). A dotted path such as
  `realm_access.roles` is followed when no top-level claim has that exact name.
- **A required role.** When `required_role` is set and you do not have it, sign-in is
  refused: "Your Acme account does not have the keeper role. Ask your administrator."

**The forge leg (`oauth` mode).** keeper runs a second sign-in against the forge, in the
same browser session:
- When `signin_url` is set, keeper opens it with `{authorize_path_and_query}` filled in, so
  the forge's own identity-provider login carries you through.
- The `scope` string is sent byte for byte the same on every device and version.
- **The forge must agree who you are.** If it signs you in as someone else, the connection
  is refused and its tokens are discarded: "The forge signed you in as ana, but your keeper
  account is tgorka."
- **When the forge rejects the token.** A rejected request (`401`) is refreshed once and
  retried. A second `401`, or a `403`, asks you to reconnect the repository.

## The config repository

### Layout

This is the contract between keeper and the repository. Directories whose names start with
`_` are not people.

```text
keeper-config.git/
  _template/
    keeper.toml                  # copied to <login>/keeper.toml on a person's first sign-in
    settings.toml                # optional: seeds a person's first settings.toml
    class/
      desktop.toml               # copied to <login>/keeper.<device>.toml for a new desktop
      tablet.toml                #   … for a new iPad
      mobile.toml                #   … for a new phone
    settings/
      desktop.toml               # optional: seeds settings.<device>.toml when no other desktop of yours has one
      tablet.toml                #   … iPad
      mobile.toml                #   … phone
  tgorka/                        # one directory per person, named by their login
    user.toml                    # who this directory belongs to
    keeper.toml                  # your pinned settings, every device
    keeper.macbook.toml          # your pinned settings, this device only
    settings.toml                # your preferences, every device (keeper keeps it in step)
    settings.macbook.toml        # your preferences, this device only (keeper keeps it in step)
    drives.toml                  # the drives you use, and on which devices
    bots.toml                    # your bot providers and their bots
    matrix.toml                  # your Matrix accounts
    devices/
      macbook.toml               # name, class, platform, created
      iphone-3f2a.toml
```

`user.toml`, as keeper writes it:

```toml
login = "tgorka"
display_name = "Tomasz Gorka"
sub = "283746519283746777"     # the field named by identity_field
issuer = "https://id.acme.dev"
created = "2026-09-23T10:12:00Z"
```

A repository whose `user.toml` records `zitadel_id` instead of `sub` works with
`identity_field = "zitadel_id"`. keeper never rewrites an existing `user.toml` to migrate
it.

### What keeper does there

1. **Resolve.** keeper looks up `<login>/` for your login and reads `user.toml`. Its
   identity field must equal your sign-in's `sub`. When `user.toml` also records `issuer`,
   that must match too.
2. **A mismatch stops everything.** No settings from the repository load, and the account
   shows as blocked with a sentence naming the file: "This sign-in belongs to someone else:
   tgorka/user.toml records a different account. Settings from the repository were not
   loaded." keeper never loads another person's directory.
3. **First sign-in** (no `<login>/`): write `user.toml`, copy `_template/keeper.toml` to
   `<login>/keeper.toml`, commit and push.
4. **A new device:** write `<login>/devices/<device>.toml`. When
   `_template/class/<class>.toml` exists, copy it to `<login>/keeper.<device>.toml`. Commit
   and push.
5. **A later launch** finds everything present and creates nothing.
6. **Every sync** merges your settings files and your lists of drives, bot providers and
   Matrix accounts. It rewrites and pushes them only when their content changed (see
   *Your settings, drives and accounts travel*).

**The rules keeper keeps:**
- **Create-only, except five files.** A file that already exists is never re-copied or
  rewritten, apart from the five files keeper keeps in step, directly in your `<login>/`:
  `settings.toml`, `settings.<device>.toml`, `drives.toml`, `bots.toml` and
  `matrix.toml`. `user.toml`, `keeper.toml`, `keeper.<device>.toml` and `devices/*.toml`
  are never rewritten. At one of the five names, a symbolic link, a folder, or a path
  under a file is refused, never written through.
- **Your directory only.** Only paths under your own `<login>/` are ever written or staged.
  Any other path, and any absolute path or path with `..`, is refused before it reaches
  git.
- **Concurrent pushes.** If someone else pushed first, keeper fetches, plans its writes
  again on the new tip, and tries again, up to three times. Create-only files that now
  exist are skipped, and the settings files are merged again against what just arrived.

**Device names and classes come from keeper, never from a token:**
- **The name.** On desktop it is the short hostname, lower-cased to `[a-z0-9-]` (at most 32
  characters). On iOS, where there is no hostname, it is the model plus four characters
  (`iphone-3f2a`), kept across launches. You can edit it on the confirmation sheet before
  the device is first registered.
- **Renaming later.** Settings › Account › *Rename* moves this device's files (its record,
  `keeper.<device>.toml` and `settings.<device>.toml`) in one commit.
- **The class.** `desktop` on macOS, Linux and Windows. On iOS, `tablet` on an iPad and
  `mobile` otherwise.

### Transport

- **Clone and fetch** use keeper's own git engine, with the credential supplied per request.
  No credential helper is ever consulted or written.
  - In `same` mode with `scheme = "basic"`, the access token is the password and `username`
    is the user name (`oauth2` by default). With `scheme = "bearer"`, keeper sends it as
    `Authorization: Bearer`.
  - In `oauth` mode the forge's own token is used.
- **Push** uses keeper's built-in smart-HTTP push on every platform. The `git` program is
  never involved, so the token never appears in a command line.
- **The clone** lives in keeper's data directory at `account/<id>/repo`. On macOS that is
  `~/Library/Application Support/dev.tgorka.keeper/account/<id>/repo`. It is not a drive, and
  it appears in no drive list, status or tray line.

## Where the account's settings sit

The layer files, later wins, per key:

```text
~/.keeper/keeper.toml                  user, every machine
~/.keeper/keeper.<host>.toml           user, THIS machine
<clone>/<login>/keeper.toml            account, every device
<clone>/<login>/keeper.<device>.toml   account, this device
<main>/.keeper/keeper.toml             the main sync folder, every machine
<main>/.keeper/keeper.<host>.toml      the main sync folder, THIS machine
<folder>/.keeper/keeper.toml           that folder only
<folder>/.keeper/keeper.<host>.toml    that folder, this machine
```

**Your repository wins over this machine's hand edits in `~/.keeper`.** The main sync
folder's files and per-folder files keep their authority over it.

`settings.toml` and `settings.<device>.toml` are not in this list, because they are not
layer files. Their values are applied into this device's own settings, the place the
Settings pane writes, so every file above still pins over them (see *Your settings,
drives and accounts travel*).

The account's two layer files use the same format as `~/.keeper/keeper.toml`: a `[settings]` table
with the keys listed in `docs/settings-keys.md`.
- **`mainSyncFolder`** is refused in them, with the existing fault ("only `~/.keeper/` may
  elect the main folder"). Elect the main folder in `~/.keeper/`.
- **`[folder]`** is refused. These files are in no folder.
- **A machine-local key** is accepted only in `keeper.<device>.toml`.
- **Unknown keys and wrong shapes** produce the same per-key faults as in any layer file. The
  rest of the file still applies.

**When a change takes effect.** The account's layer files are live: when a sync brings in a
change to your files, keeper swaps them in without a restart. A key read every time it is
used takes effect at once. A key read only at launch, such as hotkeys and the debug log,
takes effect at the next launch.

**How Settings shows it.** The *Set by a file* badge names the account and the repository
file (`acme: tgorka/keeper.toml`), and its hint ends by saying that settings read at startup
follow at the next launch. Faults in the account's files appear in the same list as the
other layer files' faults.

## Your settings, drives and accounts travel

Your preferences, the drives you sync, your bot providers and your Matrix accounts follow
you from device to device through your directory in the config repository. keeper keeps
five files there in step with your devices. They are the only files it ever rewrites.

### The two settings files

- **`<login>/settings.toml`** holds the preferences that are the same on every device:
  the recording format, notifications, the undo-send window, the voice phrases, the
  embedding model, and so on. These are the keys under *Keys a file may set* in
  `docs/settings-keys.md`, plus `notes.embedding_model`.
- **`<login>/settings.<device>.toml`** holds this device's machine-local settings: its
  hotkeys, which drive is its notes vault and which its tasks ledger, and where it
  records. These are the keys under *Keys only `keeper.<host>.toml` may set*, apart from
  `sync.git_path`.

```toml
# keeper keeps this file in step with your devices. Edit it freely; keeper merges it key by key.
# Values here are preferences, not locks — keeper.toml is where a setting is pinned.
[settings]
"recording.codec" = "hevc"
"notify.previews_enabled" = true
```

**Values here are preferences, not locks.** keeper applies them into this device's
settings, the same place the Settings pane writes, so every control stays editable.
Changing one in the app writes it back to the file and pushes it. To lock a setting, put
it in `keeper.toml` or `keeper.<device>.toml`, as before. **A pin still wins** over a
synced value on every device, and its control shows *Set by a file*.

You can also edit the two files by hand in the forge. keeper merges them key by key at the
next sync. A key it does not know, written by a newer keeper for example, is kept as it
is and not applied.

### Who wins

keeper remembers what each file looked like when this device last synced it. At each
sync, key by key:
- **Unchanged here since the last sync:** the repository's value applies here. This
  includes a key the repository deleted.
- **Changed only here:** the change is pushed.
- **Changed both here and in the repository:** this device's value wins, because the
  device syncing now is the last one to reach the repository (D-26). "Last" means last to
  sync: a device that comes back after a week offline wins with its week-old change.

A conflict is decided for a whole value. Two devices that change different parts of one
JSON value (the embedding model, the service file names) do not merge: one of them wins
(DW-300).

A pulled value takes effect as soon as the sync ends, just like a change made in Settings.
The exception is the hotkeys: they are registered at launch, so a synced hotkey follows at
the next launch (DW-301).

### Your first device, and a device that joins

- **Your first device.** No settings file exists yet. keeper starts `settings.toml` from
  `_template/settings.toml` and `settings.<device>.toml` from
  `_template/settings/<class>.toml`, when the operator provides them. What you already set
  on this device wins over the template, and the template fills in the rest. With no
  template, the files start from this device's own settings.
- **A device that joins.** `settings.toml` already exists, so the new device **pulls** it:
  the repository's values win over whatever the new device had. A setting only the new
  device has is added to the file.
- **The joining device's own file.** A new device has no `settings.<device>.toml`. keeper
  seeds it from your **latest device of the same class**: of your other desktops (or
  tablets, or phones), the one whose settings file changed most recently in the
  repository. That device's values win, so a new Mac starts with your last Mac's hotkeys
  and vault choice. With no other device of the class, keeper uses
  `_template/settings/<class>.toml`, and this device's own values win. With neither, the
  file starts from this device's own settings.
- **Renaming a device** moves its settings file along with its other two files. The
  device keeps its settings and is not seeded again.

### What never travels, and why

- **Secrets.** No token, password, session, keychain item or credential value is ever
  written to the repository. A drive or a bot provider records only *which* credential
  it uses: `account`, `own` (a token or key in this device's keychain) or `none`.
- **`sdk_encryption`.** Whether keeper encrypts its local store is tied to this
  device's keychain passphrase. Changing it re-keys this device, so it is not a
  preference another device could hand over.
- **`sync.git_path`.** It names a `git` program on this disk.
- **Session-state keys.** Last choices, one-time answers and bookkeeping belong to this
  install. Examples: which notes you have read, window placement, the first-run answer,
  the account's device name and last sync time, and whether a drive or provider uses the
  account. These are the rest of *Keys no file may set* in `docs/settings-keys.md`. The
  lists of drives and bot providers do record whether each one uses the account, but
  only as a description an offer can suggest. No file ever sets that choice.

### Drives, bots and folders are named, not numbered

A vault, a ledger drive, a recording destination, the voice target and the embedding
model's provider are each stored as an id that keeper mints separately on every device.
The settings files carry a reference instead:

| Reference | Used by |
| --- | --- |
| `drive:<remote URL>#<branch>` | `notes.active_vault`, `tasks.ledger_vault`, `recording.destination_profile_id` |
| `provider:<kind>:<base URL>` | the provider inside `notes.embedding_model` |
| `bot:<kind>:<base URL>#<target>` | `bots.voice_target` |

Remote and base URLs are compared in a normalized form: the scheme and host lower-cased, a
trailing `/` or `.git` dropped, and any user name or password removed. So
`https://Git.Acme.dev/tgorka/notes.git/` and `https://git.acme.dev/tgorka/notes` are one
drive.

A reference that does not resolve on this device, because you have not added that drive
or bot here yet, stays in the file and is not applied. It does not count as a change made
here, so this device never deletes it or pushes its own value over it. Once you add the
drive or bot, the next sync applies it. A recording folder (`recording.destination_dir`)
works the same way: it applies only when that folder exists on this device.

### Drives, bot providers and Matrix accounts are offered

Three more files list what you use, and on which devices:
- **`drives.toml`:** each drive's name, remote URL and branch; its roles (notes,
  recordings, sessions, tasks) and their subfolders; the policy a synced folder may carry
  (excludes, the LFS threshold, virtual-file patterns and size floor, the release time,
  tags, the commit subject); and whether it uses your account, its own token, or none.
  The local folder, the direction and the other per-device choices stay on each device.
- **`bots.toml`:** each bot provider's kind, name, base URL and read timeout, whether it
  uses your account or its own key, and its pinned bots (target, name, order and look).
- **`matrix.toml`:** each Matrix account's user id, its homeserver, and how it signs in
  (password, SSO or Beeper).

Each entry names the devices that use it. Adding one on a device adds that device, and
removing it removes that device. An entry no device uses leaves the file. Two devices
have the same drive when its remote and branch match (DW-302), the same provider when its
kind and base URL match, and the same Matrix account when its user id matches.

**Nothing is added by itself.** A drive needs a folder on this device, a provider needs a
key unless it uses the account, and a Matrix account needs you to sign in. keeper
therefore *offers* what your other devices use and this one does not, under **From your
account**:
- **Settings › Sync** lists each drive with its host, its roles and the devices that
  have it. *Add…* opens the add-folder form filled in, and you choose the folder (on iOS
  keeper assigns it, as usual). When the drive uses your account and the account is
  signed in, the account is already chosen.
- **Settings › Bots** lists each provider with its kind, host and bots. When it uses your
  account and the account is usable, *Add* adds it and its bots in one step. Otherwise
  *Add…* opens the provider form filled in and asks for its key.
- **Adding a Matrix account**, and the first-run step, show one button per account. It
  fills in the homeserver and user name on the Password tab, or opens the Beeper tab for
  a Beeper account (DW-305). You then sign in as usual.

Each block appears only when it has something to offer. An offer stays until you add it
here, or until no other device uses it (DW-304). Settings › Account sums it up in one
line, for example: "Your settings sync with this account. Last synced 14:02. On your
other devices: 2 drives, 1 bot provider, 1 Matrix account."

### When it syncs

Besides launch, focus and *Sync now* (see *Syncing, and being offline*), a change that
travels starts a sync by itself: changing a synced setting, or adding or removing a drive,
a bot provider, a bot or a Matrix account. The 15-minute limit does not hold it back.
Changes made while a sync runs go in the next one. Offline, they wait and are pushed at
the next sync that reaches the repository. Values a sync applies never start another
sync, and without an account none of this happens. Each such commit reads
`tgorka: settings from macbook`.

### Who can change your settings

keeper writes only in your own directory. The repository's permissions decide who else
can. In a repository where everyone can write everywhere, anyone with access can edit
your settings files, just as they can edit your `keeper.toml`, and keeper applies what it
finds (DW-303).

Signing out, or forgetting the account, stops the syncing. Values that already synced
stay, as this device's own settings.

## Syncing, and being offline

keeper syncs the config repository:
- at launch;
- when its window gains focus, at most once every 15 minutes;
- when you choose *Sync now* in Settings › Account, which is never throttled;
- after you change something that travels (see *When it syncs*), which the 15-minute
  limit does not hold back.

**At launch, before anything reads a setting,** keeper applies your settings from the last
clone on disk, with no network. An unreachable network therefore gives you yesterday's
settings, not none.

Once you are signed in:

| Status | What it means |
| --- | --- |
| *Up to date.* | The last sync reached the repository. |
| *Offline — using settings from 14:02.* | The repository or the network could not be reached. The last clone's settings stay applied. keeper tries again at the next focus, launch or *Sync now*. |
| *Sign in again to keep your settings in sync.* | The identity provider rejected the stored sign-in, for example after a long idle period or a revoked session. This is different from being offline. |
| *Blocked: …* | keeper refused to apply the repository's settings, for example because `user.toml` records someone else, or a required role is missing. The sentence says why. |

The sentences above are examples of keeper's wording, which Rust composes. An account
status line beside the sync status shows only *offline*, *sign in again* and *blocked*.
Nothing modal interrupts you.

**Nothing about an unreachable network deletes, rewrites or disables a local setting.** If
the identity provider cannot be reached when you first sign in, keeper says so and stays
local-only.

## Signing out, and forgetting the account

**Sign out…** (Settings › Account):
- revokes your refresh token(s), when the provider has a revocation endpoint;
- deletes the keychain items, whatever the network does;
- opens the provider's sign-out page in the same browser session.

The account's layer files (`keeper.toml` and `keeper.<device>.toml`) stop applying, and
nothing syncs. Values that already synced into this device's settings stay, as if you had
set them here. **The clone and `account.toml` are kept**, so signing in again needs no
link. The files in the repository are untouched.

**Forget this account…** is available whenever an account is set up, signed in or not. It:
- signs out, if you are signed in;
- deletes `account.toml`;
- deletes this device's copy of the repository (`<data>/account/<id>/`);
- stops applying the account's layer files, and forgets which version of your settings
  files this device last synced. Values that already synced stay, as if you had set them
  here.

Settings › Account then shows the paste field again. **The repository on the server, and
every other device and person, are untouched.**

## The account as a credential for drives and bots

When an account is signed in, two forms offer **Use my {name} account**:
- **The drive form.** The drive then asks for the account's current token on each
  operation, instead of storing a pasted token. The token field disappears, and no
  per-drive keychain item is created.
- **A bot provider's form.** The provider sends `Authorization: Bearer {access token}`.

Both are opt-in, per drive and per provider. Nothing switches over by itself, and choosing
the keychain again brings the token field back.

**How a drive sends the token.** A drive sends it the way keeper already sends a drive
token: as the Basic user name with an empty password on git, as Basic `token:` for LFS, and
as `token {token}` for the forge's API. It does not use `config.auth`'s scheme. Gitea,
Forgejo and oauth2-proxy accept that form. GitLab documents only the password form.

**In `oauth` mode** a drive receives the sign-in token, not the forge's token. A drive on a
forge that accepts only its own tokens cannot use the account.

**Every service you point at the account must accept the provider's token.** The provider
must therefore include that service in the token's audience, through `extra_scopes`. A
token that names several services can be replayed at any of them, so keep the list short.

## Security notes

- **Setup links show hosts.** A link or QR code can name any identity provider and any
  repository. keeper shows both hosts in full before anything is written, and writes nothing
  until you continue.
- **Descriptors hold no secrets.** keeper is a public client, and a `client_secret`
  anywhere in a descriptor is refused. The client id, issuer and repository URL in a
  descriptor are not secret. Sharing the QR code grants no access.
- **HTTPS only.** Every URL in a descriptor must be `https`, apart from loopback hosts for
  testing. A descriptor is fetched with no redirects and at most 64 KiB.
- **Tokens live in the keychain only.** A signed-in account keeps its tokens in one keychain
  item, `account/<id>/session`, plus `account/<id>/forge` in `oauth` mode, under the service
  `dev.tgorka.keeper`. On macOS that means at most one keychain prompt per item per launch.
  On iOS the items are this-device-only and never synchronised.
- **Where tokens never go.** Tokens are never sent to keeper's webview, written to a log or
  a file, put in a command line or a URL, or committed to the config repository.
- **Nothing secret travels in your settings.** The settings files and the lists of
  drives, bot providers and Matrix accounts carry values and descriptions only: never a
  token, password, session, keychain item or credential. A drive or provider records
  only which credential it uses (`account`, `own` or `none`).
- **Refresh tokens stay on their device.** Each device holds its own refresh token.
  Rotation is handled by one refresher per device, and a rotated token is stored before
  the new access token is used.
- **Your directory, and only yours.** keeper writes only under your own `<login>/`. It
  creates files and never rewrites them, apart from the five files it keeps in step (see
  *Your settings, drives and accounts travel*), and it loads a directory only when its
  `user.toml` names your sign-in.
  The config repository cannot redirect the account: the descriptor lives outside the layer
  files, so no file in the repository can name another identity provider or repository.
- **Roles come from verified identity only.** Roles are read from the verified ID token or
  from UserInfo whose subject matches, never from the access token.
- **Disclosed destinations.** While an account is set up, Settings › About lists its hosts
  (the sign-in host, the host the descriptor came from, the repository's host, and in
  `oauth` mode the forge's host) with keeper's other destinations. With no account the list
  is unchanged. See `docs/egress.md`.

## Operator notes

These come from the providers' own documentation and source. The epic's research document
cites each one (`research-account-2026-09-23.md` §6–§8). Where a behaviour was inferred and
not tested live, it says so.

**Any OIDC provider**
- **Register keeper as a public, native client.** Use PKCE S256, no secret, and the redirect
  URI registered exactly. Register `keeper://oauth/<id>/forge/callback` too if the forge is
  the same provider.
- **Keep `offline_access`** in the scopes (it is in the default). Without it most providers
  issue no refresh token, and the account needs a new sign-in whenever the access token
  expires.
- **Sign with an asymmetric algorithm:** RS256/384/512, PS256/384/512, ES256, ES384 or
  EdDSA. HS256 cannot be verified by a public client, and ES512 is not supported.
- **Copy the issuer exactly,** trailing slash included, as it appears in the provider's
  discovery document.
- **If the provider rejects `keeper://…`,** use a loopback redirect on desktop
  (`http://127.0.0.1/callback`). RFC 8252 asks for reverse-DNS private-use schemes, and a
  strict provider may refuse a scheme with no period.

**Zitadel**
- **Audience.** The ID token's audience always includes the project id, so add it to
  `trusted_audiences`.
- **Roles.** Request `urn:zitadel:iam:org:projects:roles` and
  `urn:zitadel:iam:org:project:id:<projectid>:aud`, and set `roles_claim` to
  `urn:zitadel:iam:org:project:<projectid>:roles`. It is an object whose keys are the roles.
  Roles reach the ID token only with *User Roles Inside ID Token* switched on; otherwise
  keeper reads them from UserInfo.
- **Refresh tokens** expire after 30 days idle and 90 days absolute by default. A device
  unused for longer asks for a new sign-in.

**Keycloak**
- **Roles.** They are added to the access token only by default. Switch on the roles
  mapper's *Add to ID token* (or *Add to userinfo*) and use `realm_access.roles`, or add a
  *Group Membership* mapper for `groups`. Its full-path option, on by default, gives values
  like `/team/ops`.
- **Offline tokens** survive a user logging out. keeper revokes its refresh token at sign-out
  for that reason.

**authentik**
- **Issuer.** It is per application and ends with a slash.
- **Signing key.** Select one. Without it, authentik signs with HS256, which keeper cannot
  verify.
- **Groups.** The `profile` scope carries a flat `groups` list.

**Authelia**
- **Groups** are delivered in UserInfo by default.
- **Custom-scheme redirects.** Whether Authelia accepts them at runtime is unverified, so use
  loopback on desktop if it refuses.

**The forge in `oauth` mode (Gitea, Forgejo)**
- **The app.** Create a non-confidential OAuth2 application with the redirect URI
  `keeper://oauth/<id>/forge/callback`. PKCE is required for public clients.
- **Pin `scope` and never change it.** A changed scope string makes an existing grant fail
  ("a grant exists with different scope"). The fix is for each person to revoke keeper
  under the forge's *Settings › Applications* and connect again.
- **Consent.** Forgejo shows its consent page on every sign-in for a public client.
- **Multiple devices on Forgejo.** Forgejo invalidates refresh tokens by default
  (`[oauth2] INVALIDATE_REFRESH_TOKENS = true`). With one grant per person and app, a
  sign-in or refresh on one device invalidates the forge refresh token on the others,
  which then ask to reconnect the repository. Setting it to `false` avoids that. This is
  inferred from Forgejo's code and not yet tested live. Gitea's default is `false`.
- **`signin_url`** is `https://<forge>[/<subpath>]/user/oauth2/<auth-source-name>?redirect_to={authorize_path_and_query}`.
  People signing in to the forge for the first time land on its link-account page unless
  `[oauth2_client] ENABLE_AUTO_REGISTRATION` is on.

**The repository in `same` mode.** The repository has to accept the identity provider's
access token on git over HTTPS. Two ways exist today:
- **oauth2-proxy in front of Gitea or Forgejo,** with `--skip-jwt-bearer-tokens` and the
  forge's reverse-proxy authentication. It reads the token from Bearer or Basic.
- **Forgejo 16's *Authorized Integrations*.** These are configured per user, and the token's
  audience must be exactly the one Forgejo generates.

Gitea and GitLab accept only their own tokens.

**Access.** Every person needs clone and push access to the config repository. keeper
itself writes only inside each person's own directory, whatever the forge allows.
Anyone who can push can therefore also edit another person's settings files, and keeper
applies them as it applies that person's own changes.

**Settings templates.** `_template/settings.toml` seeds a person's first `settings.toml`.
`_template/settings/<class>.toml` seeds a device's `settings.<device>.toml` when none of
that person's other devices of the same class has one. Both are optional and use the
`[settings]` format of the settings files.

## Files and keys

| What | Where |
| --- | --- |
| The descriptor | `~/.keeper/account.toml` (desktop); `account.toml` in the app's data directory (iOS) |
| The clone | `<data>/account/<id>/repo`, removed by *Forget this account* |
| The session | keychain `account/<id>/session` (service `dev.tgorka.keeper`) |
| The forge session (`oauth` mode) | keychain `account/<id>/forge` |
| Last sync time, this device's name | settings keys `account.<id>.last_synced_ms`, `account.<id>.device_slug` (keeper-owned; see `docs/settings-keys.md`) |
| A drive or provider using the account | settings keys `sync.credential_source.<profile_id>`, `bots.provider_credential_source.<provider_id>` = `account` |
| Your preferences | `<login>/settings.toml` (every device), `<login>/settings.<device>.toml` (this device) |
| Your drives, bot providers and Matrix accounts | `<login>/drives.toml`, `<login>/bots.toml`, `<login>/matrix.toml` |
| Settings templates (optional) | `_template/settings.toml`, `_template/settings/<class>.toml` |
| What this device last synced | settings keys `account.<id>.settings_base.shared`, `account.<id>.settings_base.device` (keeper-owned; cleared by *Forget this account*) |
| Setup link | `keeper://setup?descriptor=…` / `keeper://setup?d=…` |
| Sign-in redirects | `keeper://oauth/<id>/callback`, `keeper://oauth/<id>/forge/callback` (Matrix's `keeper://oauth/callback` is separate) |

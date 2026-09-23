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
- what happens offline, at sign-out and when you forget the account;
- the security properties it keeps.

The design reasoning lives in `_bmad-output/planning-artifacts/`:
`epic-82-an-optional-account-and-a-config-that-follows-you.md` for the decisions and the
requirement numbers below, and `research-account-2026-09-23.md` for the evidence.

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
  template on your first sign-in, adds each device you use to it, and applies the settings
  in it as layer files.

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
3. **A paste.** Paste either link, or a bare `https://…json` URL, into the field in
   Settings › Account, or into the first-run step *Sign in with an organisation account*.
   This covers a Mac, which cannot scan a QR code.

keeper then shows **one confirmation sheet**:
- the account's name;
- **the sign-in host and the repository host**, in full;
- this device's name, which you can edit until the device is registered, and its class.

**Nothing is written before you choose *Continue*.** *Cancel* writes nothing. A setup link
can point keeper at anyone's identity provider, which is why both hosts are always on the
screen. Continue only for hosts you recognise.

After *Continue*, keeper:
1. writes `account.toml`;
2. signs you in (see *Signing in*);
3. in `oauth` mode, connects the forge;
4. clones or fetches the config repository;
5. creates or registers what is missing in your directory;
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
    class/
      desktop.toml               # copied to <login>/keeper.<device>.toml for a new desktop
      tablet.toml                #   … for a new iPad
      mobile.toml                #   … for a new phone
  tgorka/                        # one directory per person, named by their login
    user.toml                    # who this directory belongs to
    keeper.toml                  # your settings, every device
    keeper.macbook.toml          # your settings, this device only
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
5. **A later launch** finds everything present and writes nothing.

**The rules keeper keeps:**
- **Create-only.** A file that already exists is never re-copied or rewritten.
- **Your directory only.** Only paths under your own `<login>/` are ever written or staged.
  Any other path, and any absolute path or path with `..`, is refused before it reaches
  git.
- **Concurrent pushes.** If someone else pushed first, keeper fetches, re-applies its
  create-only writes onto the new tip (skipping files that now exist), and tries again, up
  to three times.

**Device names and classes come from keeper, never from a token:**
- **The name.** On desktop it is the short hostname, lower-cased to `[a-z0-9-]` (at most 32
  characters). On iOS, where there is no hostname, it is the model plus four characters
  (`iphone-3f2a`), kept across launches. You can edit it on the confirmation sheet before
  the device is first registered.
- **Renaming later.** Settings › Account › *Rename* moves both of this device's files in one
  commit.
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

The account's two files use the same format as `~/.keeper/keeper.toml`: a `[settings]` table
with the keys listed in `docs/settings-keys.md`.
- **`mainSyncFolder`** is refused in them, with the existing fault ("only `~/.keeper/` may
  elect the main folder"). Elect the main folder in `~/.keeper/`.
- **`[folder]`** is refused. These files are in no folder.
- **A machine-local key** is accepted only in `keeper.<device>.toml`.
- **Unknown keys and wrong shapes** produce the same per-key faults as in any layer file. The
  rest of the file still applies.

**When a change takes effect.** The account's files are live: when a sync brings in a
change to your files, keeper swaps them in without a restart. A key read every time it is
used takes effect at once. A key read only at launch, such as hotkeys and the debug log,
takes effect at the next launch.

**How Settings shows it.** The *Set by a file* badge names the account and the repository
file (`acme: tgorka/keeper.toml`), and its hint ends by saying that settings read at startup
follow at the next launch. Faults in the account's files appear in the same list as the
other layer files' faults.

## Syncing, and being offline

keeper syncs the config repository:
- at launch;
- when its window gains focus, at most once every 15 minutes;
- when you choose *Sync now* in Settings › Account, which is never throttled.

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

The account's settings stop applying. **The clone and `account.toml` are kept**, so signing
in again needs no link. The files in the repository are untouched.

**Forget this account…** is available whenever an account is set up, signed in or not. It:
- signs out, if you are signed in;
- deletes `account.toml`;
- deletes this device's copy of the repository (`<data>/account/<id>/`);
- stops applying the account's settings.

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
- **Refresh tokens stay on their device.** Each device holds its own refresh token.
  Rotation is handled by one refresher per device, and a rotated token is stored before
  the new access token is used.
- **Your directory, and only yours.** keeper writes only under your own `<login>/`, creates
  and never rewrites, and loads a directory only when its `user.toml` names your sign-in.
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

## Files and keys

| What | Where |
| --- | --- |
| The descriptor | `~/.keeper/account.toml` (desktop); `account.toml` in the app's data directory (iOS) |
| The clone | `<data>/account/<id>/repo`, removed by *Forget this account* |
| The session | keychain `account/<id>/session` (service `dev.tgorka.keeper`) |
| The forge session (`oauth` mode) | keychain `account/<id>/forge` |
| Last sync time, this device's name | settings keys `account.last_synced_ms`, `account.device_slug` (keeper-owned; see `docs/settings-keys.md`) |
| A drive or provider using the account | settings keys `sync.credential_source.<profile_id>`, `bots.provider_credential_source.<provider_id>` = `account` |
| Setup link | `keeper://setup?descriptor=…` / `keeper://setup?d=…` |
| Sign-in redirects | `keeper://oauth/<id>/callback`, `keeper://oauth/<id>/forge/callback` (Matrix's `keeper://oauth/callback` is separate) |

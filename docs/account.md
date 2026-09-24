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
- how your settings, drives, bot providers and Matrix accounts travel between your devices,
  and how a reinstalled device restores itself;
- how one sign-in serves your drives, your bots and your Matrix account;
- how to browse your repositories on your account's forge and on GitHub, and add them as
  drives, including GitHub access through your organization's broker;
- what happens offline, at sign-out and when you forget the account;
- the security properties it keeps.

The design reasoning lives in `_bmad-output/planning-artifacts/`:
`epic-82-an-optional-account-and-a-config-that-follows-you.md` for the decisions and the
requirement numbers below, `epic-84-your-settings-follow-you.md` for the settings that
travel, `epic-85-your-device-comes-back-and-one-sign-in-opens-everything.md` for restoring a
device and the one sign-in, `epic-86-browse-your-repositories-and-add-them-as-drives.md` for
browsing repositories and the GitHub broker, and `research-account-2026-09-23.md` for the
evidence.

## The one idea

**An account is a descriptor, a sign-in, and a directory.**

- **The descriptor** is a small file that says who signs you in and where your settings live.
  An operator writes it once for everyone. It reaches a device through one input: a link, a
  QR code, or a pasted URL.
- **The sign-in** is your organisation's own login page, in the system's sign-in window.
  keeper never sees your password. The token it gets back is kept in the OS keychain and is
  a general credential: the config repository uses it, and so can a drive or a bot provider
  if you choose. The same sign-in session also opens your Matrix account when your
  homeserver trusts the same provider (see *One sign-in: drives, bots and Matrix*).
- **The directory** is `<your login>/` in the config repository. keeper creates it from a
  template on your first sign-in and adds each device you use to it. It applies the pins
  in it as layer files, keeps your preferences, drives, bot providers and Matrix accounts
  there in step with every device, and restores a reinstalled device from it.

**keeper works fully without an account.** With no descriptor nothing changes: no file is
read that can fail, no network request is made, and Settings › Account shows only a paste
field and a sentence saying so. The one feature that can work without an account is
browsing GitHub through keeper's own GitHub app, and only once that app is registered and
you connect it (see *Browse your repositories*).

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
5. **Optionally, repository sources** for *Browse repositories…*: a GitHub broker
   (`[github_broker]`) or a GitHub OAuth App (`[[forges]]`). See *Repository sources in the
   descriptor* and *GitHub through your organization's broker*.

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

### Repository sources in the descriptor

Two optional tables tell *Browse repositories…* where else to look (see *Browse your
repositories*). Neither belongs to the sign-in. Adding, editing or removing them never
signs anyone out and never replaces the account. In the JSON form they are
`"github_broker": {"url": …, "app": …}` and `"forges": [{"kind": …, …}]`.

```toml
[github_broker]                   # GitHub access through your organization's broker
url = "https://broker.acme.dev"   # https; keeper calls <url>/v1/whoami and <url>/v1/token
app = "acme-bot"                  # optional: the GitHub App to prefer when two grants cover one owner

[[forges]]                        # optional; any number
kind = "github"                   # github only; a forgejo entry is refused (below)
id = "github"                     # [a-z0-9-]{1,32}; default: "github"
name = "GitHub"                   # shown on the source switcher; GitHub's default
web_base = "https://github.com"   # GitHub's default
api_base = "https://api.github.com"   # GitHub's default; must be api.github.com for github.com
client_id = "…"                   # a public OAuth App's client id, for Connect GitHub
```

- **The account's own forge needs no entry.** In `oauth` mode, with `config.api_base` set,
  keeper lists the forge your settings repository lives on, with the forge sign-in it
  already holds. It is shown under the account's `name`.
- **`[github_broker]`** makes GitHub a source for everyone signed in to the account. The
  `github` source then gets its tokens from the broker. A GitHub OAuth App (a `client_id`
  here, or keeper's own) is used only when the broker has no grants for the person.
- **A `github` entry** is needed only for a `client_id`, a `name`, or a second GitHub
  source under another `id`. The broker serves only the source whose id is `github`; any
  other GitHub source connects with its own `client_id`. An entry keeper cannot get a
  token for is not shown. keeper's own GitHub app is used only by a source on github.com's
  own two addresses, `https://github.com` and `https://api.github.com`.
- **The API stays with its forge.** A source whose `web_base` is github.com must use
  `api_base = "https://api.github.com"`, and any other source must keep its `api_base` on
  its `web_base`'s host. The API is where keeper sends the source's token, so a descriptor
  cannot point it anywhere else.
- **Only the account's own Forgejo is listed.** A `forgejo` entry is refused: "keeper lists
  only the account's own Forgejo; remove this [[forges]] entry." keeper has no way to get
  a token for a second Forgejo: Forgejo has no device flow, the broker serves GitHub only,
  and your account's forge token never goes to another host.

**What keeper refuses.** The whole descriptor is refused, with a sentence, as for any
other fault:
- a `web_base`, `api_base` or broker `url` that is not `https` (loopback hosts excepted, for
  tests), or that carries a user name, a password, a query or a fragment;
- a client secret, anywhere, as always;
- two sources with one `id`, or an entry with the id `account-forge`, which belongs to the
  account's own forge;
- a `forgejo` entry;
- a github.com `web_base` whose `api_base` is not `https://api.github.com` ("`forges[N].api_base`
  must be https://api.github.com for github.com; keeper sends a GitHub token only to
  GitHub."), or any other `web_base` whose `api_base` is on another host;
- with `[github_broker]`, a `github` entry that points anywhere but `github.com`: the
  broker's tokens are github.com's. github.com is compared as an address, so a trailing
  `/`, upper case or an explicit `:443` is still github.com.

**The setup sheet shows every host that will get a token.** Besides the sign-in host and
the repository host, it lists the broker's host (*Gets GitHub access from `<host>`*) and
each repository source's hosts, so nothing receives a token that you did not see before
*Continue*.

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
- with a `[github_broker]`, the line **Gets GitHub access from `<host>`**, and with
  repository sources, every host that will receive a repository token, so you see where
  your tokens can go before you continue.

**Nothing is written before you choose *Continue*.** *Cancel* writes nothing. A setup link
can point keeper at anyone's identity provider, which is why both hosts are always on the
screen. Continue only for hosts you recognise.

If this install has already registered this device for the account (you are setting the
same account up again), the sheet says so: "This device is already in your settings
repository; keeper will restore its drives, bots and settings." A freshly installed keeper
cannot know this yet, because it has no copy of the repository. It finds out at its first
sync, keeps the name, and restores itself (see *A device that comes back*).

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
   up to date (see *Your settings, drives and accounts travel*). On this install's first
   sync, it also restores this device from its file (see *A device that comes back*);
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
    device.macbook.toml          # this device's drives, bots and accounts, so it can be restored (keeper writes it)
    drives.toml                  # the drives you use, and on which devices
    bots.toml                    # your bot providers and their bots
    matrix.toml                  # your Matrix accounts
    devices/
      macbook.toml               # name, class, platform, created, machine (a fingerprint, below)
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
   Matrix accounts, and describes this device in its own file. It rewrites and pushes them
   only when their content changed (see *Your settings, drives and accounts travel*).

**The rules keeper keeps:**
- **Create-only, except six files.** A file that already exists is never re-copied or
  rewritten, apart from the six files keeper keeps in step, directly in your `<login>/`:
  `settings.toml`, `settings.<device>.toml`, `drives.toml`, `bots.toml`, `matrix.toml`
  and `device.<device>.toml`. Only the device a `device.<device>.toml` names ever writes
  it. `user.toml`, `keeper.toml`, `keeper.<device>.toml` and `devices/*.toml` are never
  rewritten. At one of the six names, a symbolic link, a folder, or a path under a file
  is refused, never written through.
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
- **A reinstall keeps its name.** A device record written by this version of keeper carries
  `machine`, a fingerprint: the SHA-256 of the operating system's machine id together with
  your sign-in's `sub`. It recognises this machine for you and reveals no hardware id. When
  your directory already records a device with the name this device would take, of the
  same class and platform, and that record carries this machine's fingerprint or no
  fingerprint at all, keeper takes that name again instead of adding a suffix, and the
  device restores itself from its files. Anything else gets a four-character suffix
  (`macbook-3f2a`) and starts afresh: another class, another platform, or another
  machine with the same host name (DW-311). A record written before the fingerprint
  existed can be taken by any machine with that name, class and platform (DW-317). An
  iPhone or iPad has no machine id and draws a new name when keeper is reinstalled, so it
  does not restore itself (DW-318). On Linux and Windows, reinstalling the operating system
  gives a new machine id; reinstalling keeper does not.
- **Renaming later.** Settings › Account › *Rename* moves this device's files (its record,
  `keeper.<device>.toml`, `settings.<device>.toml` and `device.<device>.toml`) in one
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

`settings.toml`, `settings.<device>.toml` and `device.<device>.toml` are not in this list,
because they are not layer files. The settings files' values are applied into this
device's own settings, the place the Settings pane writes, so every file above still pins
over them (see *Your settings, drives and accounts travel*).

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
six files there in step with your devices. They are the only files it ever rewrites. One
of them describes this device so completely that a reinstalled device restores itself
from it.

### The two settings files

- **`<login>/settings.toml`** holds the preferences that are the same on every device:
  the recording format, notifications, the undo-send window, the voice phrases, the
  embedding model, and so on. These are the keys under *Keys a file may set* in
  `docs/settings-keys.md`, plus `notes.embedding_model`.
- **`<login>/settings.<device>.toml`** holds this device's own settings: its hotkeys, which
  drive is its notes vault and which its tasks ledger, where it records, which `git`
  program it uses, whether its local store is encrypted, whether it listens for a phrase
  and in which language, the notes list's last choices, and whether each drive and bot
  provider uses your account. These are the keys under *Keys only `keeper.<host>.toml`
  may set*, plus the few that describe this device (see *What travels from this
  device, and what never does*).

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
- **A reinstalled device** keeps its name (see *A device that comes back*), so it finds
  its own `settings.<device>.toml` and pulls it, rather than seeding from another device.
- **Renaming a device** moves its settings file along with its other files. The device
  keeps its settings and is not seeded again.

### What travels from this device, and what never does

- **Secrets never travel.** No token, password, session, keychain item or credential
  value is ever written to the repository. A drive or a bot provider records only *which*
  credential it uses: `account`, `own` (a token or key in this device's keychain) or
  `none`.
- **The settings that describe this device travel in `settings.<device>.toml`,** and each
  applies only where it can:
  - **`sdk_encryption`** decides how keeper protects the local store of a Matrix account
    added after it. An account already on the device keeps the store it has, because
    changing that would re-key it (DW-315);
  - **`sync.git_path`** applies only where that program exists on this disk;
  - **`bots.voice_locale`** applies only where the recogniser can run that language.
    Otherwise it is refused, never replaced;
  - **`bots.wake_enabled`** can travel off, but never on (see *Listening is yours to
    switch on*);
  - **`notes.hide_service_files`** and **`notes.include_private`**, the notes list's last
    choices;
  - **whether each drive and bot provider uses your account**, as
    `"sync.credential_source.drive:<remote>#<branch>" = "account"` or
    `"bots.provider_credential_source.provider:<kind>:<base URL>" = "account"`. A drive or
    provider that uses the keychain has no entry. The value applies only as the account
    you are signed in to, and only to a drive or provider this device has. Otherwise it
    stays in the file and waits, like any reference (below). A drive added from a
    repository source records that source instead, as `"forge:<source id>"` (for example
    `"forge:github"`). That value names a source, never a token. A device applies it only
    to a drive whose remote is on that source's host, and only when it can get that
    source's token itself: it holds a GitHub connection for that source, or the source is
    served by the broker and you are signed in. Otherwise the value stays in the file,
    unapplied, and the drive keeps signing in as it did (see *Adding repositories as
    drives*).
- **Per-install state stays on the device.** Which notes you have read, where the capture
  window sits and its draft, the notes you just created, one-time answers (the first-run
  answer, the iOS sync notice), and the account's bookkeeping (this device's name, the
  last sync, what it last synced, whether it restored itself). Chat pins and drafts stay
  here too (DW-306, DW-307). These are the rest of *Keys no file may set* in
  `docs/settings-keys.md`, and keeper.db's own tables.

### Drives, bots and folders are named, not numbered

A vault, a ledger drive, a recording destination, the voice target and the embedding
model's provider are each stored as an id that keeper mints separately on every device.
The settings files carry a reference instead:

| Reference | Used by |
| --- | --- |
| `drive:<remote URL>#<branch>` | `notes.active_vault`, `tasks.ledger_vault`, `recording.destination_profile_id` |
| `drive:<remote URL>#<branch>^<name>` | the same, when two of this device's drives share a repository and branch |
| `provider:<kind>:<base URL>` | the provider inside `notes.embedding_model` |
| `bot:<kind>:<base URL>#<target>` | `bots.voice_target` |

Remote and base URLs are compared in a normalized form: the scheme and host lower-cased, a
trailing `/` or `.git` dropped, and any user name or password removed. So
`https://Git.Acme.dev/tgorka/notes.git/` and `https://git.acme.dev/tgorka/notes` are one
drive.

Two drives of one repository and branch, such as `tgdrive` and `tgdrive-light`, are told
apart by name: their references end in `@tgdrive` and `@tgdrive-light`. A drive alone on its
repository keeps the short form. A reference without a name goes to the only drive of that
repository here, or else to the one whose name sorts first. Renaming one of two such
drives changes its reference for your other devices (DW-316).

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
removing it removes that device. An entry no device uses leaves the file. Two devices have
the same drive when its remote, branch and name match, the same provider when its kind and
base URL match, and the same Matrix account when its user id matches. A drive record
written before names counted still matches when exactly one drive here has its remote and
branch.

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
  a Beeper account (DW-305), and you then sign in as usual. An account that signs in with
  single sign-on starts that sign-in on the same click (see *One sign-in: drives, bots
  and Matrix*).

Each block appears only when it has something to offer. An offer stays until you add it
here, or until no other device uses it (DW-304). Settings › Account sums it up in one
line, for example: "Your settings sync with this account. Last synced 14:02. On your
other devices: 2 drives, 1 bot provider, 1 Matrix account."

### This device's own file

`<login>/device.<device>.toml` describes this device so that it can be rebuilt:
- **every drive**, whole: its folder, remote and branch, direction, which parts of the
  repository it keeps, its LFS and virtual-file settings, its roles and templates, its
  watch windows, its recordings policy, and its schedules (kind, schedule, mode, on or off).
  Only keeper's internal id for the drive and the disk it was last seen on are left out;
- **every bot provider**: kind, name, base URL, read timeout, whether it uses your account
  or its own key, its bots, and the folders each bot may reach;
- **every Matrix account**: user id, homeserver, how it signs in, its colour, its incognito
  choice and the networks you muted.

Only this device writes the file, only when something in it changed, and in the same
commit as your settings files. It carries no secret: a drive's remote is written without
any user name or password, and a credential is only `account` or `own`. It starts with a
comment saying that keeper rewrites it from the device, so edit `settings.<device>.toml`
instead. `drives.toml` lists the portable half of each drive, for your *other* devices to
offer. This file restores *this* device, so it keeps this device's own folders.

### A device that comes back

The first time a new install of keeper syncs your account and finds a
`device.<device>.toml` for its name, it restores that device, once. A reinstall on the
same machine finds its old name (see *Device names and classes*, under *The config
repository*):
1. **Settings** arrive as they do for any device (see *Your first device, and a device
   that joins*), including which drives and providers use your account.
2. **Drives** that are not on this device are added with their folder, every setting and
   their schedules. When a folder's parent does not exist yet (a disk that is not plugged
   in), that drive waits: Settings › Account says "Waiting for /Volumes/Field to restore
   Field recordings.", and keeper restores it at the first sync after the disk is back. On
   iPhone and iPad keeper places the folder itself, as usual.
3. **Bot providers** are added with their bots and the folders each bot may reach. A
   provider that uses your account works at once. One with its own key asks for the key.
   Permission for a folder that is still waiting waits with the folder.
4. **A Matrix account** that signs in with single sign-on starts one sign-in, which you
   finish (see *One sign-in: drives, bots and Matrix*). Accounts that sign in with a
   password or through Beeper are offered on the sign-in screen, as before. Its colour,
   incognito choice and muted networks apply when you add it.

Settings › Account then says what came back, for example: "Restored 2 drives, 1 bot
provider and your settings from your account." Nothing already on the device is changed
or added twice.

**Once.** After that first restore, this device's own state is the truth, and keeper
writes it to the file at every sync. A drive you remove afterwards stays removed. A drive
removed on another device is not removed here (DW-309). To restore again, forget the
account and set it up again (DW-310).

### Listening is yours to switch on

Whether keeper listens for your wake phrase (*Listen for a phrase*) travels with the rest
of this device's settings, but a sync or a restore never switches it on. Switching it off
travels. Switching it on is only ever your tap, on the device that will listen. When a
restore finds that listening was on for this device, Settings › Account says "Listening
for your wake phrase was on for this device." and offers **Turn listening on**. The button
checks the switch first, and asks for the microphone by name, as the switch does. No
layer file can set it either: `bots.wake_enabled` in a `keeper.toml` is refused, as
`sdk_encryption` is.

### When it syncs

Besides launch, focus, *Sync now* and the daily pull (see *Syncing, and being offline*),
a change that travels starts a sync by itself: changing a synced setting, or adding or
removing a drive, a bot provider, a bot or a Matrix account. The 15-minute limit does not
hold it back.
Changes made while a sync runs go in the next one. Offline, they wait and are pushed at
the next sync that reaches the repository. Values a sync applies never start another
sync, and without an account none of this happens. Each such commit reads
`tgorka: settings from macbook`.

### Who can change your settings

keeper writes only in your own directory. The repository's permissions decide who else
can. In a repository where everyone can write everywhere, anyone with access can edit
your settings files, just as they can edit your `keeper.toml`, and keeper applies what it
finds (DW-303).
That includes whether a drive or bot provider uses your account, which travels in your
device settings file: someone who can push could switch one of your drives to it.

Signing out, or forgetting the account, stops the syncing. Values that already synced
stay, as this device's own settings.

## Syncing, and being offline

keeper syncs the config repository:
- at launch;
- when its window gains focus, at most once every 15 minutes;
- when you choose *Sync now* in Settings › Account, which is never throttled;
- after you change something that travels (see *When it syncs*), which the 15-minute
  limit does not hold back;
- once a day while keeper runs, even with its window closed (see *Keeping up in the
  background*).

**At launch, before anything reads a setting,** keeper applies your settings from the last
clone on disk, with no network. An unreachable network therefore gives you yesterday's
settings, not none.

Once you are signed in:

| Status | What it means |
| --- | --- |
| *Up to date.* | The last sync reached the repository. |
| *Offline — using settings from 14:02.* | The repository or the network could not be reached. The last clone's settings stay applied. keeper tries again at the next focus, launch, daily pull or *Sync now*. |
| *Sign in again to keep your settings in sync.* | The identity provider rejected the stored sign-in, for example after a long idle period or a revoked session. This is different from being offline. |
| *Blocked: …* | keeper refused to apply the repository's settings, for example because `user.toml` records someone else, or a required role is missing. The sentence says why. |

The sentences above are examples of keeper's wording, which Rust composes. An account
status line beside the sync status shows only *offline*, *sign in again* and *blocked*.
Nothing modal interrupts you.

**Nothing about an unreachable network deletes, rewrites or disables a local setting.** If
the identity provider cannot be reached when you first sign in, keeper says so and stays
local-only.

## Keeping up in the background

While keeper runs on a desktop, it pulls the config repository at least once a day, also
while its window is closed. Closing the window hides keeper, and it keeps running. When a
day has passed since the last sync attempt, or since launch if there has been none, keeper
starts one sync in the background, the same sync as *Sync now*. Changes your other
devices made to your settings, drives and accounts therefore reach this device within a
day, even if you never open it.

- It rides keeper's existing once-a-second tick, the one that updates the menu-bar icon.
  There is no separate timer.
- It does nothing without an account, and nothing while the sync it started is still
  running.
- It does not run while keeper is quit or the Mac is asleep. keeper syncs at its next
  launch instead. On iPhone and iPad, where an app in the background is suspended, keeper
  syncs when you open it (DW-314).
- keeper-syncd, the Linux background service, does not use the account, and does not pull
  the config repository.

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
- deletes the GitHub connections of the sources the descriptor names
  (`forge/<source id>/<client id>/session`). A connection through keeper's own GitHub app
  stays until you disconnect it;
- forgets every repository list and broker token keeper holds in memory, so nothing
  fetched for this account is shown to the next one;
- stops applying the account's layer files, and forgets which version of your settings
  files this device last synced and whether it has restored itself. Setting the same
  account up again on this install therefore restores the device again, and adds nothing
  it already has. Values that already synced stay, as if you had set them here.

Settings › Account then shows the paste field again. **The repository on the server, and
every other device and person, are untouched.**

## One sign-in: drives, bots and Matrix

One sign-in to your organisation serves the config repository, your drives, your bot
providers and your Matrix account. Each is your choice, per drive, per provider and per
account. Nothing switches over by itself.

**Drives and bot providers.** When an account is signed in, two forms offer **Use my
{name} account**:
- **The drive form.** The drive then asks the account for a token on each operation,
  instead of storing a pasted token. The token field disappears, and no per-drive keychain
  item is created.
- **A bot provider's form.** The provider sends `Authorization: Bearer {access token}`.

Choosing the keychain again brings the token field back. The choice travels in your
device settings file, so a restored device uses the account for the same drives and
providers.

**Which token a drive gets:**
- **A drive on the forge's host.** When your repository is in `oauth` mode, a drive on the
  forge's host (or the config repository's host) gets the forge's own token, the one
  keeper already holds for the config repository. The forge accepts it for git and for
  LFS. If the forge is not connected, the drive asks you to sign in again. A token the
  forge refuses is refreshed once.
- **Any other drive** set to the account gets the sign-in token.
- **A drive added from a repository source** signs in through that source, not the
  account: see *Adding repositories as drives*.

Either token is sent the way keeper sends any drive token: as the Basic user name with an
empty password on git, as Basic `token:` for LFS, and as `token {token}` for the forge's
API. It does not use `config.auth`'s scheme. Gitea, Forgejo and oauth2-proxy accept that
form, and Forgejo reads such a user name as its own OAuth2 token too. GitLab documents only
the password form. The forge's token is only ever sent to the forge's host.

**Bot providers** always get the sign-in token, as Bearer. The provider has to accept it:
- a gateway behind your identity provider does;
- Ollama has no authentication and ignores it;
- a gateway with its own static key, such as Hermes in the owner's setup, keeps that key.
  Choose the keychain for it (DW-308).

**Every service you point at the sign-in token must accept it.** The provider must
therefore include that service in the token's audience, through `extra_scopes`. A token
that names several services can be replayed at any of them, so keep the list short.

**Matrix.** When your homeserver signs people in through the same identity provider
(single sign-on), adding your Matrix account needs no second password:
- Choose *Add account › Matrix account…*, then *Sign in with single sign-on* with your
  homeserver. Or choose the account's button under *From your account*, which starts that
  sign-in on the same click.
- On macOS and iOS, keeper opens the homeserver's sign-in in the same system sign-in window
  it used for your account. Your identity provider already knows you there, and only asks
  you to confirm. Other desktops open the default browser, and the answer comes back
  through a link.
- keeper registers with the homeserver as a native app, with the redirect
  `dev.tgorka.keeper:/oauth/callback`.
- When the homeserver maps your sign-in to the user id you already have (see *Operator
  notes*), your rooms and history are all there.
- A restored device starts this sign-in once by itself (see *A device that comes back*).
- The Matrix session stays in this device's keychain and never travels.

## Browse your repositories

*Browse repositories…* lists the repositories you can reach on your account's forge and on
GitHub, shows which of them you already sync, and adds the ones you pick as drives. It
sits beside *Add a folder* in Settings › Sync, and in the Sync pane while it is empty. It
appears only when keeper has somewhere to look:
- **your account's forge**, when your settings repository lives on a forge in `oauth` mode
  and the descriptor gives `config.api_base`;
- **GitHub**, when the descriptor names a GitHub broker (see *GitHub through your
  organization's broker*) or a GitHub OAuth App (see *Repository sources in the
  descriptor*), or once keeper's own GitHub app is registered. keeper's own app needs no
  account, but it is not registered yet, so today GitHub needs a descriptor that names one
  (DW-323).

With none of these, nothing new appears and nothing is contacted.

A switcher at the top of the sheet picks the source, and keeper remembers the last one
you used. A source keeper cannot use right now says why, in one sentence, with the one
thing that helps: *Connect GitHub*, *Sign in*, or *Try again*.

### Your account's forge

keeper lists the forge with the sign-in it already holds for your settings repository, and
asks for no new consent. It reads who you are from the forge's userinfo, then searches the
repositories you can reach:
- your own;
- those of organizations whose teams you are on;
- those you collaborate on.

According to Forgejo's code, a public repository of another person on which you can only
read is left out.

Forgejo does not say whether an owner is a person or an organization. Your own
repositories come first, as *You*, and every other owner is a group of its own (DW-328).

keeper searches rather than asking for *your repositories*, because that list needs
`read:user`, which a grant of `openid profile write:repository` lacks. Widening the grant
would make everyone revoke keeper on the forge first (see *Operator notes*).

### GitHub

What the GitHub list holds depends on where keeper's GitHub tokens come from:
- **Through your organization's broker** (see *GitHub through your organization's
  broker*): the owners the broker lets you reach, each listed with a short-lived token for
  that owner. An owner the broker cannot serve says why, and the others still list.
- **Through your own connection** (*Connect GitHub*): your repositories, those you
  collaborate on, and those of every organization you belong to.

When the descriptor names a broker, keeper uses it. Your own connection is offered only
when the broker has no grants for you and a GitHub OAuth App is configured. Having no
grants is not an outage. The sheet says "Your account has no GitHub access on
`<broker host>`. Ask its administrator to add you.", shows no older list, and offers
*Connect GitHub* when it can.

**Connecting GitHub yourself.**
- *Connect GitHub* shows a code. *Open github.com* copies it and opens GitHub's device
  page, where you paste it and approve keeper. keeper opens that page only when it is an
  https page on the source's own host or a subdomain of it. Any other page gets
  "`<host>` gave keeper an approval page elsewhere, so keeper does not open it." keeper
  waits while you approve, and *Cancel* stops waiting, even when GitHub's answer is
  already on its way: a cancelled connection is never stored. If the page cannot be
  opened or the code copied, the sheet says so, and the code stays selectable.
- keeper asks GitHub for `repo` and `read:org`. GitHub has no read-only scope for private
  repositories, and `read:org` lets keeper see your organizations.
- The connection is kept in this device's keychain (`forge/<source id>/<client id>/session`)
  and never leaves the device. Each source and OAuth App has its own item, so a
  descriptor's own GitHub app never shares, or replaces, a connection made through
  keeper's. When GitHub made the connection an expiring one, keeper refreshes it a minute
  early. Reading a connection never deletes it; only *Disconnect* does.
- **Disconnect** deletes it from the keychain, together with any item an earlier build
  kept at `forge/<source id>/session`. GitHub offers no way for an app without a secret to
  revoke a token, so if you want keeper gone on GitHub too, remove it under GitHub ›
  Settings › Applications; the sheet links keeper's own page there (DW-327).

**Organizations that hide repositories.**
- **An organization that has not approved keeper** shows only its public repositories.
  GitHub turns this restriction on by default for new organizations, and hides the rest
  without an error. When GitHub does name it, keeper says "Some organizations haven't
  approved keeper, so their private repositories are hidden." It links the page where you
  ask the organization's owners to approve keeper. Only they can (DW-326).
- **An organization that uses single sign-on** hides its repositories until you authorize
  keeper for it. keeper says "Repositories of organizations that use single sign-on are
  hidden until you authorize keeper for them.", with GitHub's link when GitHub gives one.

### The list

- **Grouped by owner:** *You* first, then organizations and other people, A to Z. Each row
  shows a lock for a private repository, the chips *Fork*, *Archived*, *Template* and
  *Mirror*, the description, when it last changed and its size.
- **What you already sync:**
  - a repository that a drive on this device syncs reads *Syncing here as `<drive>`* and
    cannot be selected;
  - one your other devices sync reads *On `<devices>`*, from `drives.toml`.

  URLs are compared normalized, as for references, so `https://GitHub.com/a/b.git` and
  `https://github.com/a/b` are one repository.
- **What keeper can only download.** A repository you can only read, an archived one and
  a mirror say so on their row: "You can only read this repository, so keeper only
  downloads it.", "This repository is archived, so keeper only downloads it." or "This
  repository is a mirror, so keeper only downloads it." keeper adds such a repository as a
  download-only drive, so it never piles up changes that GitHub or the forge would refuse.
  Through a broker, a repository counts as writable only when one of your grants gives
  write access to its contents.
- **Finding one:** search by name, owner or description; filter by owner; show forks and
  archived repositories with their switches (both off at first); sort by last update or by
  name.
- **At most 1,000 per source.** Past that, keeper says "Only the first 1,000 are listed;
  search looks only through those." (DW-329). A repository beyond them can still be
  added by its URL in *Add a folder*.
- **Fetched when you look, kept in memory.** keeper fetches a source's list the first
  time you open it and when you choose *Refresh*. It keeps the list only while keeper
  runs, only for the person who is signed in, and writes nothing about it to disk. Signing
  in, signing out, forgetting the account and setting one up all forget it. Nothing is
  fetched in the background. Offline, the sheet shows the failing request's own sentence,
  which names the host that failed (for example "Can't reach `<host>`."), followed by
  "Showing the list from `<HH:MM>`." when a list was already fetched.
- **Trying again means trying again.** *Try again*, and *Sign in* once you have signed in,
  fetch the list afresh, so a source that failed once recovers without a restart.

### Adding repositories as drives

- **One.** *Add…* on a row opens *Add a folder* filled in: the repository's name as the
  drive's name, its folder (`<new drives folder>/<name>`, see below), its clone URL, its
  default branch, *Download only* where keeper can only download it, and the sign-in its
  source gives. You can change any of them. This is also the way to add a second drive of
  one repository in another folder.
- **Which sign-ins the form offers.** keeper asks Rust for the sign-ins a drive at the
  form's remote may use (`sync_credential_choices`), each with a label and one sentence
  saying what it means, and offers only those:
  - *Use my `<account>` account* only for a repository on one of the account's own hosts
    (its sign-in, config repository and forge). For a GitHub repository it is never
    offered, and keeper refuses both to save it and to hand the account's token over:
    GitHub would be given your account's sign-in, and refuse it anyway.
  - *GitHub access through `<account>`* for a GitHub repository when the account names a
    broker: the broker gives keeper a one-hour token for that one repository, as you.
  - *Sign in with GitHub* for a GitHub repository through your own GitHub connection.
  - Change the remote to another host and the choices follow it; a choice the new remote
    cannot use goes back to the drive's own token.
- **Several.** Tick them (⌘A anywhere in the sheet, outside a text field, ticks every
  visible one not yet added) and choose *Add `<n>` drives…*. The count and the batch are
  the ticked repositories that are still listed and not yet added.
  - **Where they go.** On a desktop, new drives go in `~/keeper/git` unless you choose
    another folder under Settings › Sync › *New drives go in* (`sync.drive_folder`,
    machine-local; *Use default* goes back). The batch starts from that folder and you can
    change it for one batch. Each repository goes into `<folder>/<name>`, and you can rename
    each one. On iPhone and iPad keeper places the folders itself, as for any drive.
  - **Which folders keeper uses.** A folder must be a full path ("Choose a full folder
    path."; `~/` means your home folder). It must be absent (keeper creates it), empty
    (a Finder `.DS_Store` or folder icon does not count), or already a clone of that
    repository. A folder that holds other files, or a name used twice, blocks only its own
    row.
  - **Never on, in or around another drive.** A folder that is already a drive's folder,
    lies inside one, or contains one is refused. keeper compares the real paths, and on a
    Mac ignores the difference between upper and lower case, as the disk does.
  - **What keeper refuses.** A repository that already syncs on this device is refused,
    with "Already syncing here as `<name>`." Add a second drive of it through *Add…*.
  - **The result.** The others are added and start syncing, and each row says how it went.
    If a row fails, keeper removes the empty folders it made for it. Renaming a row clears
    its old result. One batch runs at a time.

**Which sign-in a drive added this way uses:**
- **A repository on your account's forge** uses your account, so it gets the forge's own
  token (see *One sign-in: drives, bots and Matrix*).
- **A GitHub repository** uses its source, recorded as `forge:<source id>`, for example
  `forge:github`.
  - Through a broker, the drive gets a one-hour token for its one repository: with write
    access to its contents when one of your grants allows it, and read access otherwise,
    in which case the drive only downloads.
  - Through your own connection, it gets that connection's token.
- **Only on the source's host.** keeper gives a source's token only to a drive whose
  remote is on that source's host (for GitHub, `https://github.com/…`). Any other drive
  asks you to sign in, and nothing is sent. Choosing a source for a drive on another host
  is refused: "This drive's repository isn't on `<host>`."
- **The token's spelling.** It is sent the way keeper sends any drive token: as the Basic
  user name with an empty password. GitHub accepts that for a connection's token, on git
  and LFS alike (tested). With a broker's installation token it has not been tried yet
  (DW-325).
- **It travels.** Which sign-in each drive uses travels in `settings.<device>.toml`, so a
  restored device keeps it, once it can get that source's token itself (see *What travels
  from this device, and what never does*).

## GitHub through your organization's broker

An organization can give its people GitHub access without anyone connecting a GitHub
account to keeper. A broker holds the organization's GitHub App keys, and hands short-lived
tokens to people signed in to the account. keeper speaks the protocol of makistack's
`github-broker` (tgorka/makistack#863). **That broker is an open pull request.** It is
deployed on the owner's server (tailnet only) but not merged, and it serves no token yet:
one app has no credentials, and the other is installed nowhere (DW-324). The protocol below
is the one in its source (`docker/github-broker/app/broker.py` at `7f32b70`). keeper ships
no broker; it only calls one.

**In the descriptor:**

```toml
[github_broker]
url = "https://broker.acme.dev"   # https
app = "acme-bot"                  # optional
```

- **`url`:** the broker's base address. keeper calls `<url>/v1/whoami` and
  `<url>/v1/token`.
- **`app`:** the GitHub App keeper prefers when two of a person's grants fit a request.
  keeper considers only the grants that cover the owner, the repository and the permission
  it is about to ask for, as the broker's own policy does. Among those, `app` wins, and
  without it keeper takes the first, in the order the broker lists them. So keeper never
  asks for a token the policy would refuse.
- The setup sheet shows "Gets GitHub access from `<host>`".

**What keeper sends.** Every request carries the account's sign-in access token as
`Authorization: Bearer`. The requests:
1. **`GET <url>/v1/whoami`** answers `{sub, subject, grants: [{app, owners, repositories,
   permissions}]}`, where `repositories` is `"*"` or a list of names. keeper keeps the
   answer while it runs, for the person signed in, until an error or a *Refresh*.
2. **To list,** for each owner in the grants, A to Z, at most 20 owners. Past that, keeper
   says "`<broker host>` gives you more owners than keeper lists at once; only the first 20
   are listed."
   - keeper sends `POST <url>/v1/token` with
     `{"app": "<app>", "owner": "<owner>", "permissions": {"metadata": "read"}}`, or with
     `{"contents": "read"}` when no grant covering the owner allows `metadata`;
   - with that token, it pages
     `GET https://api.github.com/installation/repositories?per_page=100`, following GitHub's
     `link` header: up to 10 pages per owner, and 60 requests (the `whoami`, each token and
     each page) and 1,000 repositories in all. Repositories are grouped by owner before
     that limit cuts, so yours come first.
   - An owner whose `owner.type` is `User` is shown as *You* (DW-328).
   - A repository is writable only when a grant covering it gives `contents: write`, and
     GitHub's own `permissions.push`, when it sends one, is not false. Anything else is
     added download-only.
3. **For a drive** on `https://github.com/<owner>/<repo>`, keeper sends `POST <url>/v1/token`
   with `{"app": "<app>", "owner": "<owner>", "repositories": ["<repo>"], "permissions":
   {"contents": "write"}}` when a grant covering it allows write, and with
   `{"contents": "read"}` otherwise. Such a drive only downloads.

The broker answers `{token, expires_at, app, owner, repositories, permissions}`. Tokens last
an hour.
- keeper keeps each one **in memory only**, keyed by the person signed in and by what it
  asked for, and serves it only while that person is still signed in.
- It asks for a new one five minutes before `expires_at`.
- Signing in, signing out, forgetting the account and setting one up forget every token
  and `whoami` answer.
- Nothing from the broker is written to the keychain or to disk.
- keeper follows no redirect with a token.

**The answers keeper handles.** The broker's errors are JSON, `{error, detail}`. The same
rules apply to `whoami`, to a listing token and to a drive's token. A failing `whoami`
lists nothing.

| Answer | What keeper does |
| --- | --- |
| 401 (any error) | "Sign in again to reach GitHub through your organization." The account's *Sign in* is offered. A 401 straight after keeper has just refreshed your sign-in is a refusal instead, "`<broker host>` did not accept your account's sign-in: `<detail>`", because signing in again would not fix it. |
| 401 from GitHub on one owner's `installation/repositories` | keeper drops that token, and that owner's notice says "GitHub didn't accept the token `<broker host>` gave keeper for `<owner>`; refresh to try again." The other owners still list. |
| 404 `not_installed` | That owner's notice: "keeper's GitHub app `<app>` isn't installed on `<owner>`." The other owners still list. |
| 503 `app_unconfigured` | That owner's notice: "GitHub access for `<owner>` isn't set up on `<broker host>` yet." The other owners still list. |
| 403 `forbidden` (the policy), or 403 `github_refused` (the installation does not cover it) | That owner's notice: "`<broker host>` refused `<owner>`: `<detail>`." The other owners still list. |
| `github_refused` with a 5xx status (GitHub itself failing) | The source is unreachable: "GitHub isn't answering `<broker host>` right now." |
| An owner no grant names (a GitHub drive under another owner, for example) | "`<broker host>` gives you no access to `<owner>`'s repositories." Nothing is asked of the broker. |
| `whoami` with `subject: null`, or no grants | No access, a state of its own: "Your account has no GitHub access on `<broker host>`. Ask its administrator to add you." No older list is shown. When a GitHub OAuth App is configured, *Connect GitHub* is offered instead. |
| 400 `bad_request` | The source is unreachable: "keeper couldn't list GitHub's repositories." A 400 means keeper asked wrongly, so the answer is a fault to report, not a retry. |
| Any other 5xx (503 `idp_unavailable`, 502 `broker_error`) | The source is unreachable: "`<broker host>` can't answer right now.", with *Try again*. |
| Any other status | "`<broker host>` refused keeper (HTTP `<status>`)." |
| An answer keeper cannot read | "The GitHub broker sent an answer keeper could not read." |
| No answer at all | "Can't reach `<host>`.", naming the host that failed, with *Try again*. The broker is often on a private network, such as a tailnet, that a device away from it cannot reach. |

**What the broker decides, and what GitHub decides.**
- **The broker's policy** says which apps and owners each person may use, keyed by the
  identity provider's `sub`. Each grant is a ceiling.
- **The GitHub App's installations** are a second ceiling. GitHub refuses a repository the
  app is not installed on, or a permission it was not granted.
- **So the list is not "every organization you belong to".** It is the owners your grants
  name, where the app is installed.
- **An installation token acts as the app, not as you.** GitHub's `/user` endpoints refuse
  it, which is why keeper lists `installation/repositories` owner by owner.

**Running one.** makistack's runbook covers it (`docs/runbooks/github-broker.md` on branch
`feat/github-broker`). What keeper relies on:
- **The keys stay in the broker.** It holds each GitHub App's private key and never hands
  one out. It mints installation tokens narrowed to what was asked for.
- **It verifies keeper's token.** keeper's access token is checked against the identity
  provider's JWKS, with the issuer exact and the audience one the broker accepts. For
  keeper, accept the project whose id keeper requests as an audience
  (`urn:zitadel:iam:org:project:id:<id>:aud` in `extra_scopes`).
- **The token must be a JWT.** The identity provider must issue keeper JWT access tokens,
  as makistack's ZITADEL does for keeper.
- **Grants are keyed by `sub`,** so a renamed login cannot inherit someone else's grants.
- **It serves https at its final address.** keeper refuses plain http, except to a
  loopback host, and follows no redirect with a token.

## Security notes

- **Setup links show hosts.** A link or QR code can name any identity provider and any
  repository. keeper shows both hosts in full before anything is written, and writes nothing
  until you continue.
- **Descriptors hold no secrets.** keeper is a public client, and a `client_secret`
  anywhere in a descriptor is refused. The client id, issuer and repository URL in a
  descriptor are not secret. Sharing the QR code grants no access.
- **HTTPS only.** Every URL in a descriptor must be `https`, apart from loopback hosts for
  testing. A descriptor is fetched with no redirects and at most 64 KiB.
- **Tokens live in the keychain or in memory only.** A signed-in account keeps its tokens in
  one keychain item, `account/<id>/session`, plus `account/<id>/forge` in `oauth` mode. A
  GitHub connection has its own, `forge/<source id>/<client id>/session`. All of them are
  under the service `dev.tgorka.keeper`. On macOS that means at most one keychain prompt
  per item per launch. On iOS the items are this-device-only and never synchronised. A
  broker's tokens and the repository lists are kept in memory only, for the identity that
  fetched them, and are forgotten at every sign-in, sign-out, *Forget this account* and
  setup.
- **Where tokens never go.** Tokens are never sent to keeper's webview, written to a log or
  a file, put in a command line or a URL, or committed to the config repository.
- **A forge token goes only to its forge.** A repository source's token is sent only to
  that source's own host (its API, which must sit on the same host, or `api.github.com` for
  github.com; or git and LFS on its web host) or to the descriptor's broker, only over
  https (loopback hosts excepted, for tests), and never after a redirect. A drive whose
  remote is on another host gets no token from that source, and cannot be set to use it.
  The setup sheet lists every such host before anything is written.
- **Nothing secret travels in your settings.** The settings files, the lists of drives,
  bot providers and Matrix accounts, and this device's own file carry values and
  descriptions only: never a token, password, session, keychain item or credential. A
  drive's remote is written without any user name or password. A drive or provider
  records only which credential it uses (`account`, `own` or `none`). A drive added from a
  repository source records only the source's name, `forge:<source id>`.
- **Only you arm the microphone.** Listening can travel off, never on, and no layer file
  may set it. Switching it on is a tap on the device that will listen.
- **The machine fingerprint is yours only.** A device record's `machine` is a SHA-256 of
  the machine id and your sign-in's `sub`. The same machine gives another person a
  different value, and the machine id cannot be read back from it.
- **Refresh tokens stay on their device.** Each device holds its own refresh token.
  Rotation is handled by one refresher per device, and a rotated token is stored before
  the new access token is used.
- **Your directory, and only yours.** keeper writes only under your own `<login>/`. It
  creates files and never rewrites them, apart from the six files it keeps in step (see
  *Your settings, drives and accounts travel*), and it loads a directory only when its
  `user.toml` names your sign-in.
  The config repository cannot redirect the account: the descriptor lives outside the layer
  files, so no file in the repository can name another identity provider or repository.
- **Roles come from verified identity only.** Roles are read from the verified ID token or
  from UserInfo whose subject matches, never from the access token.
- **Disclosed destinations.** While an account is set up, Settings › About lists its hosts
  (the sign-in host, the host the descriptor came from, the repository's host, and in
  `oauth` mode the forge's host) with keeper's other destinations. GitHub's hosts join the
  list while GitHub is in use as a repository source, and a broker's host while the
  descriptor names one. With no account and no GitHub source the list is unchanged. See
  `docs/egress.md`.

## Operator notes

These come from the providers' own documentation and source. The epic's research document
cites each one (`research-account-2026-09-23.md` §6–§8). The notes on listing repositories
and on GitHub cite epic 86's *Verified facts, with sources*. Where a behaviour was inferred
and not tested live, it says so.

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
  which then ask to reconnect the repository. Drives that use the account on the forge's
  host ride the same token, so they stop with it, and the forge token lasts only an hour.
  Setting the option to `false` avoids all of that (DW-292, DW-312). This is inferred from
  Forgejo's code and not yet tested live. Gitea's default is `false`.
- **`signin_url`** is `https://<forge>[/<subpath>]/user/oauth2/<auth-source-name>?redirect_to={authorize_path_and_query}`.
  People signing in to the forge for the first time land on its link-account page unless
  `[oauth2_client] ENABLE_AUTO_REGISTRATION` is on.
- **Listing repositories.** *Browse repositories…* reads `GET {issuer}/login/oauth/userinfo`
  for the person's id, then `GET {api_base}/repos/search?uid=<id>&private=true`, paged by
  number against `api_base` (Forgejo's `Link` header names `ROOT_URL`). Both work with a
  `write:repository` grant. `/user/repos` and `/user/orgs` would need `read:user` and
  `read:organization`, so keeper does not use them, and a working `scope` need not change
  for browsing.

**GitHub, for *Connect GitHub***
- **Register an OAuth App**, not a GitHub App, and switch on *Enable Device Flow*. keeper
  sends no client secret and never needs one: the device flow and its refresh work
  without it. Put the client id in a `[[forges]]` `github` entry (see *Repository sources
  in the descriptor*), and leave `web_base` and `api_base` at GitHub's defaults. Never put
  the secret in a descriptor, which refuses it.
- **Scopes.** keeper asks for `repo read:org`. GitHub has no read-only scope for private
  repositories.
- **Organizations.** GitHub turns OAuth App access restrictions on by default for new
  organizations. Until an owner approves the app, its private repositories are hidden
  from everyone who connects through it.
- **Tokens per person.** GitHub keeps at most 10 tokens per person, app and scope, and an
  eleventh revokes the oldest. Each device that connects holds one.

**GitHub, through a broker:** see *GitHub through your organization's broker*.

**The Matrix homeserver, for single sign-on**
- **OAuth for Matrix.** The homeserver must offer OAuth 2.0 for Matrix (MSC3861):
  keeper discovers it through `auth_metadata` or `auth_issuer` (MSC2965), registers itself
  dynamically as a native client, and signs in with PKCE. A homeserver without it is told
  apart from one that cannot be reached, and keeper says it does not offer single sign-on.
- **Registration.** keeper registers with `client_uri` `https://keeper.tgorka.dev/`,
  `application_type: native` and the redirect `dev.tgorka.keeper:/oauth/callback`: the
  host of `client_uri`, reversed. A homeserver that checks native redirects refuses a
  private-use scheme that is not that reversal, which is why keeper does not use
  `keeper://` here.
- **The identity provider.** Make it the homeserver's upstream provider, and map its
  `preferred_username` to the Matrix localpart, so people keep the user ids they have.
  Keep registration closed if only existing users should sign in. Passwords and bots'
  access tokens keep working beside it.
- **Tuwunel** (the owner's makistack), configured through env only:
  - `TUWUNEL_IDENTITY_PROVIDER` (makistack #860) sets the brand, the client id and secret
    from 1Password, the issuer, `trusted`, `userid_claims = ["preferred_username"]` and
    registration off;
  - `TUWUNEL_WELL_KNOWN__CLIENT` turns on the built-in OIDC server (makistack #861). Its
    issuer is the homeserver's own origin with a trailing slash, and dynamic client
    registration is open, which keeper needs;
  - ZITADEL gets a confidential web app for it, registered with
    `scripts/zitadel/register-oidc-app.sh`.

  On a Tuwunel 1.8.1 trial, `dev.tgorka.keeper:/oauth/callback` registered and
  `keeper://oauth/callback` was refused. See makistack's Matrix runbook.

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
| A GitHub connection (*Connect GitHub*) | keychain `forge/<source id>/<client id>/session` |
| Last sync time, this device's name | settings keys `account.<id>.last_synced_ms`, `account.<id>.device_slug` (keeper-owned; see `docs/settings-keys.md`) |
| A drive or provider using the account | settings keys `sync.credential_source.<profile_id>`, `bots.provider_credential_source.<provider_id>` = `account:<id>`; in `<login>/settings.<device>.toml` as `sync.credential_source.<drive reference>`, `bots.provider_credential_source.<provider reference>` = `"account"` |
| A drive that signs in through a repository source | settings key `sync.credential_source.<profile_id>` = `forge:<source id>`; in `<login>/settings.<device>.toml` as `sync.credential_source.<drive reference>` = `"forge:<source id>"` |
| A broker's tokens, repository lists | memory only, for as long as keeper runs |
| Your preferences | `<login>/settings.toml` (every device), `<login>/settings.<device>.toml` (this device) |
| Your drives, bot providers and Matrix accounts | `<login>/drives.toml`, `<login>/bots.toml`, `<login>/matrix.toml` |
| This device, so it can be restored | `<login>/device.<device>.toml` (written only by that device) |
| Settings templates (optional) | `_template/settings.toml`, `_template/settings/<class>.toml` |
| What this device last synced | settings keys `account.<id>.settings_base.shared`, `account.<id>.settings_base.device` (keeper-owned; cleared by *Forget this account*) |
| Whether this device has restored itself, and what waits | settings keys `account.<id>.restored`, `account.<id>.restore_pending`, `account.<id>.restore_matrix_started` (keeper-owned; cleared by *Forget this account*) |
| Setup link | `keeper://setup?descriptor=…` / `keeper://setup?d=…` |
| Sign-in redirects | `keeper://oauth/<id>/callback`, `keeper://oauth/<id>/forge/callback`; Matrix single sign-on: `dev.tgorka.keeper:/oauth/callback` |

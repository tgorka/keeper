# Egress surface

keeper is a **client only** — it has no server-side components. Optional observability
contacts PostHog only after the relevant local consent or an explicit synthetic study start.
This document is the canonical, diffable record of every network destination keeper contacts,
and it is enforced in two ways:

1. **In the app.** Settings → About renders the *live* egress list, computed in Rust from your
   actual signed-in accounts, folder-sync profiles, model providers and local observability
   policy (`egress::compute_egress`, wired through `egress_list`). Entries derive from
   the same accounts, profiles, providers and consent/study state that control those
   features, rather than a hand-maintained account or folder list. The `api.beeper.com`
   and update-endpoint hosts are fixed constants surfaced by that live state
   (and single-sourced — see below).
2. **In releases.** The release workflow emits a per-release "Egress diff note" that diffs this
   file against the previous tag into the job summary (NFR-11, AD-23), so any change to where
   keeper sends traffic is visible on every release.

## What keeper connects to

| Destination | When | Why |
| --- | --- | --- |
| Each account's **Matrix homeserver** (e.g. `https://matrix.example.org`) | One entry per distinct homeserver you are signed into (duplicates collapse to one) | All Matrix protocol traffic — sync, sending, media, key backup, verification. |
| **`api.beeper.com`** | Only when at least one account is a Beeper account (by provider tag **or** by homeserver host `matrix.beeper.com`) | Beeper's unofficial email-code login and account service. Appears exactly once. |
| Each sync profile's **git remote host** (e.g. `github.com`, `forgejo.example.org`) | One entry per distinct remote *host* across your folder-sync profiles (duplicates collapse to one); absent entirely for a profile whose remote is a local path or a pendrive, which reaches no network | Fetch, push and LFS transfer for that folder. Only the **host** is shown — never the repository path, the username, or a credential that a badly-stored profile put in the URL. See `egress::remote_host`. |
| Each configured **model provider's host** (e.g. `localhost`, `127.0.0.1`, `gw.example.org`) | One entry per distinct provider *host* across Settings › Notes › *Model providers* (duplicates collapse to one), labelled *Notes search provider*; absent entirely when no provider is configured, which is how keeper ships | Embeddings for notes search, the model list its picker reads, and *Test*, against the Hermes, Ollama or OpenAI-compatible (`openai`) endpoint you typed. Only the **host** is shown — never the `/v1` path, a profile prefix, or a credential. See the provider chapter below, and `egress::remote_host`. |
| Each **organisation account destination** (e.g. `id.acme.dev`, `git.acme.dev`) | Only while an account is set up in Settings › Account (`~/.keeper/account.toml`); absent entirely without one, which is how keeper ships. One entry per distinct host the account's descriptor names: its identity provider and any endpoint it overrides, the address the setup link was read from (while keeper still knows it), the settings repository and its `api_base`, and in `oauth` mode the forge's own sign-in | Sign-in, token refresh and sign-out against the identity provider; fetch and push of your own directory in the settings repository; reading the descriptor once when you paste or open a setup link (HTTPS only, no redirects, 64 KiB cap). The push includes **this device's log**: the newest 512 KB of `keeper.log`, cut at a line, replacing `<login>/logs/<device>.log`, where everyone with access to the repository can read it. Before it leaves, every line is scrubbed: your home folder is written as `~`, and URL user names and passwords, credential-named query and fragment parameters (`access_token`, `code`, `client_secret`, …) and `Authorization`, `Bearer` and `Basic` values as `***`. The rest of the log goes as it is — account and Matrix ids, device and microphone names, paths outside your home folder, error text. It goes once a day while **Send this device's log to your account daily** in Settings › About (`diagnostics.log_upload`) is on, which it is unless switched off, and whenever you press **Send log now** there. See `docs/account.md` § *This device's log*. Only the **host** is shown. The identity provider's discovery document may name further hosts (for example a `jwks_uri` on a CDN); those are the provider's choice and are reached only as it names them. See `egress::org_account_egress`. |
| **GitHub, for browsing repositories** (`api.github.com`; also `github.com` with a connection) | Only while GitHub is a repository source in use. While the account's descriptor names a GitHub broker (`[github_broker]`): `api.github.com`, the host the broker's tokens are sent to. While this device holds a GitHub connection made with *Connect GitHub*: that source's web host and API host, `github.com` and `api.github.com`. Absent otherwise, which is how keeper ships, since keeper's own GitHub app is not registered yet | `api.github.com`: listing the repositories the person may reach (`/installation/repositories` with a broker's token; `/user` and `/user/repos` with a connection). `github.com`: *Connect GitHub*'s device code, the wait for approval, and the refresh of an expiring connection. A GitHub drive's git and LFS traffic is the sync-remote row above. Only the **host** is shown, labelled *`<source name>` repositories* (*GitHub repositories* for the `github` source). See `egress::forge_egress` and the browsing chapter below. |
| **The GitHub broker's host** (e.g. `broker.acme.dev`) | Only while the account's descriptor names `[github_broker]`; absent without an account | `GET /v1/whoami` and `POST /v1/token`, each carrying the account's sign-in access token, to get one-hour GitHub tokens for listing and for GitHub drives. See `docs/account.md` § *GitHub through your organization's broker*. Only the **host** is shown, labelled *GitHub access broker*. See `egress::forge_egress`. |
| **`github.com/tgorka/keeper/releases/...`** (the signed-update `latest.json` endpoint) | On a cadence while the app runs (about every 6 hours, starting 2 minutes after launch) while **Update automatically** is on, and on every "Check for updates" click. Off — the switch in Settings → About, or `update.auto = false` in a config file — leaves the click as the only time it is contacted | Signed auto-updates (NFR-12). Downloads are cryptographically verified against keeper's minisign public key before installing. |
| **`*.githubusercontent.com`** (GitHub's release-asset CDN) | While an update is downloading: either one the user chose to install, or one the background updater found while **Update automatically** is on | GitHub serves release files (the update binary) from its content-delivery network, which the `github.com` release URL redirects to. Disclosed so the egress list is exhaustive, not just the check endpoint. |
| **`api.github.com/repos/tgorka/keeper/releases/latest`** (the phone tier's release check) | Only on an iPhone or Android build, and only when the person presses **Check for updates** in Settings › About; listed there instead of the desktop updater's `latest.json`, which these builds never contact | Reads the latest release's tag, unauthenticated, with a 15-second timeout and a 1 MiB cap, to say whether a newer keeper is out (story 102.9, R464). Downloads and installs nothing — a new build is installed from the Mac. |
| **The agents server you run** (e.g. `agents.example.org`, `100.64.0.7:8790`) | Only while Settings › Agents & voice › *Agents server* (`agents.server_url`) holds an address keeper contacts — keeper ships none (D-4, AD-418); one entry, its host and its port when the address names one, never its path, however many of its databases this device holds; absent when the setting is blank or refused. Before this device is paired, the one request is the pairing below — to the address set, or to the server a scanned pairing link names once you confirm it, which is listed while that request runs (`Hq::contacting`); every other request waits for the pairing | `GET /databases` at every start and after pairing (which databases this device's token opens), `POST /pair/redeem` once with the code you typed or the QR you scanned and confirmed — authenticated with that one-time code, its answer carrying this device's token (`hq/mod.rs:1039-1083`, `hq/routes.rs:300-321`) — `POST /pair/start` when you press *Add a device*, `POST /clients/{id}/revoke` when you remove a device, `PUT`/`DELETE /clients/self/push-target` on a phone when its push gateway or endpoint changes (`docs/agents.md` § *A push from the agents server*), and per database: `<base>/<database>/sync` rounds (one at least every 5 s while paired, backing off to one a minute when the server cannot be reached) and one WebSocket at `<base>/<database>/realtime`, which carries the rows and presence — who is online, and an answer as it streams; an attachment's upload (`POST …/upload-grant`, then `PUT <base>/<database>/blobs/<blobId>`) before the message that names it, and its download when you open it; and, on a phone, `GET <base>/<database>/push/item/{kind}/{id}` for the one item a push names. Every request after the pairing carries this device's token, which lives only in the keychain; the pairing carries the one-time code instead. Blob and segment URLs the server hands out are fetched only on the server's own host and port; one naming any other host is refused, so this row is the whole of it. When the address changes or is cleared, the old host stays listed until its last request has ended. Once the server refuses the token (the device was removed) keeper sends it nothing more, after a restart too, until the device is paired again |
| **The push gateway you run** (e.g. `push.example.org`, `push.example.org:8443`) | Only on a phone, only while Settings › Notifications › *Push gateway* (`push.gateway_url`) holds an address keeper registers — `https://`, no user name or password, the push path — which is how keeper does not ship; one entry, the host and the port when the address names one, labelled *Push gateway — the agents server sends it ids for this device* | keeper itself never contacts it. It sends the address to the agents server as this device's push target (the row above, iPhone, `docs/ios.md` § *Push*), and the **agents server** sends it ids only — the database, the session, the kind and the item, never words — which it relays to Apple's push service. The phone reads the item from the agents server with its own token and shows only an approval waiting for you or your own agent's answer (`docs/agents.md` § *A push from the agents server*). keeper registers no pusher with a homeserver: one an older build registered is removed from each account (DW-E108-1). Only the host is shown, never the path. See `egress::push_egress`. |
| **Your UnifiedPush server** (e.g. `ntfy.example.org`) — Android | Only on Android, only after you confirmed a UnifiedPush distributor in Settings › Notifications and it gave keeper an endpoint (kept as `unifiedpush-endpoint` in the data directory); one entry, the endpoint's host and its port when it names one, labelled with the distributor by name and package — *UnifiedPush server of ntfy (io.heckel.ntfy) — keeper asks it, once per address, whether it is a Matrix push gateway; the agents server sends it ids for this device* | Unlike the iPhone's gateway, **keeper itself contacts it**, once per endpoint: `GET https://<host>/_matrix/push/v1/notify`, no redirects followed, 10 s, answer read up to 16 KiB — asking whether it is a Matrix push gateway (`agents::unifiedpush::discover_gateway`, story 98.4). The agents server's push uses that gateway shape. When it answers so, keeper sends that same address to the agents server as this device's push target, and the **agents server** sends it ids only, which it forwards to the distributor on this device; keeper reads the item from the agents server and shows only an approval waiting for you or your own agent's answer. keeper registers no pusher with a homeserver: one an older build registered is removed from each account (DW-E108-1). The endpoint's path and query are a secret (anyone holding them can post to this device) and are never shown. See `egress::unifiedpush_egress`, `docs/android.md` § *Push*. |
| **`us.i.posthog.com`** | Only when configured and a relevant local observability category is enabled, or an explicitly started synthetic study is active; all default off | Closed diagnostic/product observations, OTLP diagnostic records/traces, separately consented public remote configuration, or content-isolated synthetic study replay/heatmaps. No raw app log export. See the consent boundary below. |

### The folder-sync daemon

`keeper-syncd` is a separate binary sharing the same profile shape. The app drives folder sync
itself (its own supervisor), so each profile's remote host is in the app's live view above; the
rows below are the destinations that are the **daemon's alone**, plus the LFS destinations that
are derived at transfer time rather than from the profile — so no live view can enumerate them
ahead of the transfer, and they are disclosed here instead.

| Destination | When | Why |
| --- | --- | --- |
| **The LFS API endpoint the server names**, for an `ssh://` or scp-style profile | The first LFS transfer for that profile, and again whenever the credential is re-derived | The LFS API is HTTP even when git is not, so keeper asks the server where it is: `git-lfs-authenticate` returns an `href`, and that `href` is what keeper contacts. **It is usually the same host, and it is not guaranteed to be.** Forgejo and Gitea build it from `AppURL`/`ROOT_URL`, a different setting from `SSH_DOMAIN`, so a deployment that serves its web UI and its git-ssh on different names discloses a *second* host here — and keeper follows it, because the server is the authority on where its own API lives. Only the scheme is checked (`http`/`https`). Which credential goes there is decided by host: the server-minted `Bearer` whenever the server supplies one, and the profile's **stored** token only when the named host equals the ssh remote's host. A named host that is not the ssh host and mints no token of its own is contacted with no credential at all — keeper never forwards a token to a host the profile did not name. |
| **A `.lfsconfig`'s `lfs.url` / `remote.origin.lfsurl`**, when the repository sets one | Every LFS transfer for that profile | The repository, not the profile, decides where its LFS server is; git-lfs reads these keys and so does keeper, and they outrank everything derived. This is the one way a profile whose git remote is a **local path or a pendrive** still reaches the network: without such a key that profile transfers objects by file copy and contacts nothing, but a repository that names an LFS server beside a path remote means it, and keeper honours it with the profile's stored token. |
| **`api.github.com/repos/tgorka/keeper/releases/latest`** | On `keeper-syncd doctor`, and on `keeper-syncd update` | The version check. Read-only, unauthenticated, and it never installs anything by itself. |
| **`*.githubusercontent.com`** | Only during `keeper-syncd update`, after you ran it | Where GitHub actually serves the release binary and its `.sha256`. |

Unlike the app, the daemon's update is **verified by checksum, not by signature**: it compares
the download against the `.sha256` published beside it. That authenticates the transfer, not the
publisher — a weaker guarantee than the app's minisign check, stated plainly rather than implied
to be equivalent.

### The agents' hosts

keeper runs no agent, so no agent's traffic is keeper's. The agents run in keeper-hq's containers
(`docs/agents.md`): what they reach — the model provider, the open web, the Paseo broker, MCP
servers — is keeper-hq's and its operator's to disclose, in keeper-hq's documentation and
makistack's `docs/runbooks/keeper-agents.md`. From keeper the only agents destination is the
agents server row above.

## Bridges add no distinct egress

Bridges (WhatsApp, Telegram, Signal, …) are Matrix **appservices** that run **server-side**,
reached *through* the homeserver. keeper's client talks to the homeserver, and the homeserver
talks to the bridge — so a bridge adds no distinct client egress. The homeserver entry already
covers it. keeper never contacts a per-bridge host directly, and the egress list never fabricates
one.

## A sync remote is disclosed as a host, never as a URL

The live list shows `forgejo.example.org`, not
`https://oauth2:ghp_liveToken@forgejo.example.org/team/notes.git`. That is deliberate, and the
omission is part of the disclosure rather than a gap in it.

A remote URL is a destination *plus* three things that are nobody else's business: the
repository path (which names what you keep and where), a username, and — in a profile whose
credential was pasted into the URL instead of stored in the keychain — a live token. Settings →
About is a screen people open precisely so they can look at it, and show it to someone else;
rendering a token there would turn the honesty surface into a credential leak. The host is the
complete answer to "where do these bytes go", so the host is the whole entry.

Two consequences follow, and both are intended:

- Two profiles on one forge are **one** entry, because they are one destination. A list that
  repeated the host would overstate the egress surface as surely as omitting it would
  understate it.
- A profile whose remote is a **local path or a pendrive** discloses **nothing at all**. It
  contacts no network, and naming a directory as a "destination" would be a fabricated network
  claim in the one place that must not fabricate. (Its LFS objects can still reach the network
  if the *repository* names an LFS server — see the `.lfsconfig` row above.)

The derivation lives in `keeper-core::egress::remote_host` and is unit-tested against every
remote form git accepts — `https`, `ssh`, `git`, the scp-like `git@host:path` shorthand,
`file:`, and bare paths — including the cases that must yield no entry and the cases that
must not leak a username or token.

### The phone reaches the same remote, and nothing else new

Since Epic 66 (AD-198, AD-204) the phone has a folder, so the sync-remote row above is
reached from a second device. It is the **same destination class, from a second device,
and no new row**: a profile on the phone names a remote host and, through it, the LFS
endpoint that server's batch API answers with — exactly what the Mac's profile for the same
remote already discloses. What the phone does on the wire, stated so this file can be
checked against the code:

| Request | Made by | Purpose |
| --- | --- | --- |
| gitoxide fetch and clone over HTTPS | `src-tauri/crates/keeper-sync/src/git/fetch.rs` (the `blocking-http-transport-reqwest-rust-tls` transport, `src-tauri/Cargo.toml:199`) | The mirror: a clone into the app container, then a fetch on open, on foreground and on pull-to-refresh (`crates/keeper/src/sync.rs:600-654`). No watcher, no supervisor, no tick. |
| `GET …/info/refs?service=git-receive-pack`, then `POST …/git-receive-pack` | `src-tauri/crates/keeper-sync/src/git/push_http.rs:127` (Story 66.5, AD-202) | The phone's push — keeper speaks `receive-pack` itself, because there is no `git` on a phone and gitoxide has no push. One pack of the objects the remote lacks; refused client-side before any byte leaves unless the remote's tip is in this copy's history (`:289-328`, AD-50). |
| The LFS batch API and the object URLs it returns | `src-tauri/crates/keeper-sync/src/lfs/batch.rs`, `lfs/hydrate.rs` | Materialising a pointer on open, and uploading the objects a phone commit stages before its push may run (`engine.rs:6679-6689`). The endpoint is derived from the remote as on the Mac; a `.lfsconfig` `lfs.url` outranks it, as the daemon row above says. |

The credential is a header and never a URL, on every one of those requests. gitoxide gets
it through its programmatic `set_credentials` callback (`fetch.rs:9-14`, `:140`), so it never
reaches a helper, an argument list or `~/.git-credentials`; `push_http` builds one
`Authorization: Basic` value through the same sensitive-header path the LFS client uses
(`push_http.rs:454-461`, `lfs/batch.rs:422`) and strips userinfo from the URL before the
request is made (`push_http.rs:434-440`); the LFS client sends its `Authorization` the same
way. Every server line a push answers with — an `ng`, a hook's stderr — passes through
`scrub_userinfo` before it becomes an error the person can read (`push_http.rs:204-207`;
NFR-26). The secret itself lives in the phone's keychain under
`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` (`crates/keeper/src/ipc.rs:841-849`).

The live list on the phone is derived the same way as the Mac's: `sync_remote_urls` reads
every profile's remote on every target since Epic 66 (`crates/keeper/src/ipc.rs:10976-10993`)
and `egress::remote_host` reduces it to the host — so a folder added on the phone appears
under Settings → About on the phone on the next open, as a host, with no cache and no
hand-written row.

What the phone does **not** reach, because the code for it is either absent or refuses
there: no `ssh` (`lfs/ssh.rs:325` and gitoxide's ssh transport both spawn one, and a phone
spawns nothing — an `ssh://` remote fails at its first sync, DW-241); no `git-lfs-authenticate`
for the same reason; none of the daemon's rows (`keeper-syncd` does not exist on the phone);
no `git` credential helper. The phone and the Mac never contact each other — the remote is
the only meeting point (D-15) — so this section adds no device-to-device destination either.

## Browsing repositories adds GitHub and the organization's broker

Epic 86 (AD-333…AD-335) lets a person list the repositories they can reach and add them as
drives. It adds the two rows above, and nothing else. What it sends, stated so this file
can be checked against the code (`keeper-core/src/forges/`, the shell's `forge_ipc.rs`):

| Request | To | When |
| --- | --- | --- |
| `GET {issuer}/login/oauth/userinfo`, then `GET {api_base}/repos/search?uid=…&private=true&limit=50&page=N` | the account's forge: hosts the organisation-account row already lists | the person opens the account's forge in *Browse repositories…*, or refreshes it |
| `GET /v1/whoami`, `POST /v1/token` | the broker's host | listing GitHub through a broker (one token per owner, at most 20 owners), and a GitHub drive's token (one per drive, reused until five minutes before it expires) |
| `GET /installation/repositories?per_page=100` | `api.github.com` | listing through a broker, per owner, at most 10 pages; a broker listing makes at most 60 requests in all |
| `POST /login/device/code`, `POST /login/oauth/access_token` | `github.com` | *Connect GitHub*: the code, then polling at GitHub's own `interval` (held between 1 and 60 seconds) only while the flow the person started is open, for at most 30 minutes; and the refresh of an expiring connection |
| `GET /user`, `GET /user/repos?…&per_page=100` | `api.github.com` | once after connecting, for the login; and listing through a connection, at most 10 pages |

- **A token is a header, never a URL.** Every token rides `Authorization`, only over
  https (loopback excepted, for tests). A response that redirects is refused rather than
  followed with the token (`keeper-core/src/forges/mod.rs`).
- **Nothing runs by itself.** A list is fetched when the person opens a source or refreshes
  it, and kept in memory for the process and for the person signed in; signing in or out,
  forgetting the account and setting one up drop it. There is no timer, and nothing about
  a list is written to disk.
- **Only hosts the setup sheet showed.** A repository source's API must sit on its own web
  host (`api.github.com` for github.com), a second Forgejo cannot be named at all, and the
  setup sheet lists every host that will receive a repository token. So no row here is a
  host the person was not shown before *Continue*.
- **The in-use test is the disclosure's own.** Whether a source is in use is decided inside
  `egress::forge_egress`, not by its caller, so the rows above and the test that pins them
  cannot drift apart.
- **Where nothing goes.** Without `[github_broker]` no broker is contacted. Without a
  GitHub source, GitHub is not contacted for browsing, and keeper's own GitHub app is not
  registered yet, so an install without a descriptor that names GitHub sends nothing to
  GitHub for this feature.
- **Connecting comes before the row.** The row for a connection is computed from the
  connection, so the device-code request and the wait for approval reach `github.com`
  while the list does not show it yet. That happens only inside the *Connect GitHub* flow
  the person started and is watching, and the row appears as soon as the connection is
  made. It leaves the list again when *Disconnect* deletes the connection.
- **The broker decides the rest.** The broker itself calls GitHub's App API
  (`/app/installations/…/access_tokens`) and the identity provider's JWKS. Those are the
  broker's destinations, not keeper's.

## A model provider is a destination you typed, disclosed as a host

Notes search asks a model for the meaning of your notes, over the OpenAI-compatible wire, and a
model lives at an endpoint (Epic 61's providers, kept for notes search when Story 108.3a removed
the ⌘9 bots). That endpoint **exists only because you typed it**: keeper ships no default
endpoint, no hosted model and no proxy of its own, so a fresh install has no provider row here at
all, and adding one in Settings › Notes › *Model providers* is the act that creates the
destination. Removing the provider removes the row on the next open, with no cache to go stale. Like the consent boundary below, this makes the destination explicit:
keeper sends only the declared feature's data to the listed server. Adding an AI
provider does not enable observability or approve AI analysis of diagnostics or replay.
The provider reasoning is recorded as `docs/decisions.md` D-4.

The row is **derived, never hand-written**. `EgressKind::ModelProvider` is computed by
`egress::compute_egress` from `providers::store::provider_base_urls`, read from the same store
notes search reads, exactly as the git-remote rows are read from the sync profiles the engine
drives (AD-53). And it is reduced by the **same** `egress::remote_host` a git remote goes
through — not a second reduction that would have to be kept in step with it — so the list shows
`gw.example.org`, not `https://gw.example.org/v1` and not `/p/<profile>/v1`. The base-URL grammar
in `providers::url` already refuses userinfo and a query string, so a credential cannot legally reach
this function from a provider row; it is stripped anyway, because Settings → About is a screen
people share and the row that leaks a token is the one written by a build whose grammar was
looser than this one. Two providers on one host are one entry, for the reason the sync-remote
chapter gives: they are one destination.

**A loopback or private-network host is the normal case here, and it is disclosed, not hidden.**
Ollama's documentation points every user at `http://localhost:11434`; Hermes' api-server binds
`127.0.0.1:8642` by default. A blocklist over private ranges — the standard SSRF advice for a
server accepting arbitrary URLs — would reject the endpoints the feature exists to reach, so
keeper answers the SSRF question the way it answers every other reach-outside question: by
disclosure plus an explicit user act. `providers::url::parse_base_url` accepts a loopback, private or
link-local host and marks it `is_private` so the surface can say which side of your network the
bytes stay on; the About list then shows `localhost` or `127.0.0.1` as a row like any other. A
row that reads `127.0.0.1` is the list telling you the bytes did not leave the machine, which is
a fact worth stating rather than a destination to suppress. What the grammar refuses is the set
of shapes that cannot be part of an honest disclosure at all: a non-HTTP scheme, userinfo, a
query string, a path beyond a profile prefix.

Because this file is diffed on every release, the release workflow's egress diff note fires on
this chapter's arrival, and that is the mechanism working as designed: a new *kind* of
destination is exactly the change the note exists to make visible. The derivation and its
reductions are unit-tested beside the git-remote cases in `keeper-core::egress` — host-only,
de-duplicated, ordered after the sync remotes and before the update endpoint, and gone entirely
when the last provider is removed.

A *spoken* question does not reach this endpoint. It is a message to an agent in the agents'
database, written through `keeper_core::hq` (`docs/agents.md` § *Talking to an agent*), and the
model that answers it is the agent's, run by keeper-hq. The lock-screen banner that says what was
heard is a local notification posted by the app process with no trigger and no push
(`crates/keeper/src/voice_notify.rs:198-213`, `UNUserNotificationCenter` in-process; no APNs, no
token), so no row is added.

Since Epic 76 (AD-264, NFR-67) the notes search index can ask this same endpoint for
**embeddings** — `POST /v1/embeddings` on the provider a person chose in Settings → Notes
search, sent from `keeper-core::providers::embed` on the same client with the same host — and it is
the *only* place a note's text ever leaves the process: one chunk of a note per input, in
batches of at most 32, and never while no embedding model is chosen. keeper ships no model,
downloads no weights, and adds no destination (huggingface.co and its kin are refused by D-4 and
recorded as rejected in `research-notes-search-2026-09-19.md` §5); with no provider row there is
no embedding, and search is words only. The row above is unchanged and no row is added.

Since Epic 89 (AD-369, NFR-121) a provider may be of a third kind, `openai`: any
OpenAI-compatible endpoint, such as a CLIProxyAPI on your own tailnet. It adds **no row of its
own**. It is a provider row like the other two — the host of the base URL you typed, through the
same `egress::remote_host`, port and path dropped — and every request it makes goes to that host:
`GET /v1/models` for *Test* and the model list (the dialect has no health or version route, and
`/v1/models` is the one route it cannot lack), and `POST /v1/embeddings` for notes search. No path
gains a profile prefix. keeper ships no default for it either (D-4): the endpoint is yours. What
the gateway does with a request — which upstream it forwards to — is the gateway's
configuration and not keeper's to disclose; keeper lists where *its* bytes go, and they go to the
host you typed. `egress::tests::an_openai_provider_discloses_exactly_its_host_once` pins the one
row.

## Screen recording adds no egress

Screen recording (the macOS recording phase, Epics 16–20) adds no network destination: the
`keeper-rec` capture sidecar and the recording UI contact **no network host**, and there is no
upload, share-link, or cloud affordance anywhere in the recording feature — recordings only ever
land in the local destination folder. A finished recording may be transcribed, on this Mac only
(D-29, next section). The per-release egress inventory diff for the recording phase is
therefore empty. Like the update endpoint, this is enforced by tests: source-scan audits fail
the build if a network API ever appears in the sidecar's Swift sources
(`keeper_rec_sidecar_sources_are_network_free` in the `keeper` crate) or a network call or an
upload, share or cloud affordance in the recording frontend (`zero-egress.test.ts`). With
diagnostics consent, the app shell (not the sidecar or the recording UI) counts a lost microphone
as one closed `keeper_recording_mic_lost` cause sent to the PostHog row already listed — see
[the observability consent boundary](#the-observability-consent-boundary); no destination is added.

## Transcription adds no egress

Epic 87 (D-29, AD-339…AD-350) transcribes recordings and any audio or video file, diarizes the
speakers and matches them against a voices bank — and it adds **no row** to the table above.

- **The engine runs in keeper's process, on this Mac.** Recognition, diarization and speaker
  embeddings are FluidAudio on the Neural Engine, through the vendored bridge at
  `tools/fluidaudio-rs/`. There is no transcription server, no NAS option and no cloud
  fallback; where the Mac cannot transcribe, nothing is transcribed.
- **The engine never fetches.** It loads a model set only from `<data_dir>/models/`, and only
  when every file is present and the set is the one the config repository names now (a
  completion marker written last by a hydration that placed everything). FluidAudio's own
  download helpers (which reach Hugging Face) are never called by the bridge.
- **Models hydrate from the account's config repository**, under `_models/` (git LFS), through
  keeper's own LFS client, from the settings repository host the organisation account row
  already lists. Unlike a drive, the LFS endpoint is derived from that remote **only**: a
  `.lfsconfig` in the config repository is never read, so the repository credential never
  reaches a host the repository names. The object URLs are the ones that endpoint's batch API
  returns. Each file is checked against its sha256. Without an account there are no models,
  and Settings › Transcription says so.
- **Transcripts, voice clips, embeddings and dictionary terms are files** — beside the media, or
  in the drive the person marked as keeping voices — and leave the Mac only as that drive's
  sync already carries its files.

Enforced by `keeper-core/tests/transcription_on_device.rs` (NFR-105): it reads
`keeper_core::transcription`, the shell's `transcribe*.rs` and the bridge's Swift and Rust off
disk and fails on any network API token, and on any FluidAudio download entry point in the
bridge. The config repository hydration (keeper-sync `config_repo`) is the one sanctioned fetch
and lives outside that set.

## Voice adds no egress

Talk mode (Epic 62 on the phone, Epic 63 on the Mac) turns speech into text, sends that text to
an agent as an ordinary message, and speaks the answer — and none of the three steps contacts a
network host the table above does not already name. The message is a row in the agents' database,
sent to the agents server row exactly as a typed message would be (`docs/agents.md` § *Talking to
an agent*). Recognition is Apple's on-device recogniser: every
request either port builds sets `requiresOnDeviceRecognition = true`, and the port checks the
recogniser's `supportsOnDeviceRecognition` before it builds one. Apple's recogniser will
otherwise fall back to Apple's servers, and that fallback would be a destination this file does
not name — so there is no server-fallback path in the code, not a disabled one. Synthesis is the
system's own voice, which needs nothing from the network. The wake phrase is matched in the same
on-device transcript and never leaves the device; it is stored as a setting, not sent anywhere.

When the device has no on-device model for its locale, keeper says so — on the phone,
*"on-device speech recognition for `pl_PL` is not on this phone — download that language under
Settings > General > Keyboard > Dictation Languages; keeper never sends your voice to a server"*,
and on the Mac the same sentence naming System Settings > Keyboard > Dictation — and does
nothing else. A recogniser that cannot run on the device at all (`supportsOnDeviceRecognition`
false) gets a distinct refusal that says no download will change it
(`keeper-core/src/voice/mod.rs:110-113`, `:145-147`). That sentence is not a fallback, it is
the refusal: the person is told which language to download and why keeper will not simply use
the network instead. Like the sidecar audit above, this is enforced by a source scan rather than
asserted: `voice_on_device` in `keeper-core`'s tests reads the voice modules of both crates off
disk — `keeper-core/src/voice/**` and every `voice*.rs` in the shell crate, picked up by prefix
so a new port cannot be left out (`keeper-core/tests/voice_on_device.rs:38-41`) — and fails the
build if a recognition request is ever built without the on-device flag, if the flag is ever set
to `false`, or if a network API appears in any voice module (NFR-50). The decision is recorded
as `docs/decisions.md` D-5.

The turn models (Epic 97, D-36) do not change that conclusion. The voice activity and end-of-turn
models are files from the account's config repository, fetched with the transcription models'
client from that repository's own LFS endpoint (*Transcription adds no egress*) — the phone now
makes that fetch too, for the turn models only, to the same host — and run on the device by
ONNX Runtime, loaded by path; nothing they are given leaves the device. ONNX Runtime's library
is downloaded from `cdn.pyke.io` by the build, never by the app
(`docs/constraints-and-limitations.md`).

### On this Mac

The macOS port (`crates/keeper/src/voice_macos.rs`, Epic 63, Story 63.4) adds no destination
either, and its position differs from the phone's in two ways worth stating: there is no
`AVAudioSession` on macOS, so keeper cannot lower other audio while it speaks and releases the
microphone instead (`voice_macos.rs:29-36`), and the microphone is held open by an App Nap
activity assertion only for as long as a capture is up (`voice_macos.rs:94-98`, `:1020-1023`,
`:1050`). Every request it builds sets the on-device flag (`voice_macos.rs:1085-1086`), after
`supportsOnDeviceRecognition` is checked (`voice_macos.rs:940-941`). Settings → About renders
the following lines, mirrored one-to-one from `MACOS_DISCLOSURE_LINES` in
`src/components/settings/about-section.tsx`; `about-section.test.tsx` reads this file from disk
and fails when the two lists differ. Edit both together or neither.

- Speech is recognised on this Mac itself and never sent to a server; a language whose on-device recogniser is not installed is refused, not uploaded.
- The microphone is open only while voice is on — after you press Talk, the hotkey or the tray item, or while it listens for your wake phrase — and the system microphone indicator shows it.
- Nothing is recorded or uploaded; what you said reaches only the agents server you configured, as text, for the agent you chose.
- keeper does not lower other audio while it listens or speaks, because macOS has no audio session that could; it stops listening while it talks instead.

The Android port (`crates/keeper/src/voice_android.rs` over `gen/android/…/VoicePlugin.kt`,
Epic 98, story 98.4) adds no destination either. Every recogniser it makes is
`SpeechRecognizer.createOnDeviceSpeechRecognizer`, after `isOnDeviceRecognitionAvailable`, fed
keeper's own capture through a pipe; the factory for a recogniser that may use a server never
appears in the app, and `voice_on_device` reads every Kotlin file of the app to keep it so. An
answer is read only by a text-to-speech voice that is installed and needs no network
(`Voice.isNetworkConnectionRequired`); a language with none is refused, as on the Apple
platforms. Android's language identifier is the system's text classifier, which may be a
vendor service, so the Android port detects no language and reads the answer in the
listening language.

### A spoken answer's language is decided on the device

Epic 64 (Story 64.2, AD-182, AD-183, AD-188) makes keeper choose a voice for the answer it reads
aloud, and that choice adds no destination. The answer's dominant language is detected by
`NaturalLanguage`'s `NLLanguageRecognizer`, a system framework with no network path: the port
allocates a recogniser, constrains it to the languages this device has voices for plus the
listening language, hands it the text and reads `dominantLanguage` back
(`src-tauri/crates/keeper/src/voice_macos.rs:1493-1518`; the same four calls on the phone,
`voice_ios.rs:1647-1670`). The constraint list is built in the core from the synthesiser's
own inventory (`keeper-core/src/voice/speech.rs:49-64`), so the text is classified only among
languages it could be spoken in, and the voice inventory it comes from is
`AVSpeechSynthesisVoice.speechVoices` read once on the device (`voice_macos.rs:1453-1472`).
**No answer text leaves the device to be classified.** Which voice speaks is then a pure
function over (detected, listening, voices) in `speech.rs:66-84`; a language with no voice is
refused with the download page named (`keeper-core/src/voice/mod.rs:165-168`), never spoken
in another language's voice and never sent anywhere to be spoken. The binding is
`objc2-natural-language 0.3.2` (`src-tauri/Cargo.toml:380`), and the on-device source scan
described above covers it: `voice_on_device` reads `voice_macos.rs` and `voice_ios.rs` off
disk and fails the build if a network API appears in either.

No request body changes for a spoken question. It is written as the words the voice turn heard,
with no metadata and no extra sentence, exactly as a typed message is
(`keeper-core/src/hq/spoken.rs:579-630`), so the agents server receives nothing a typed question
would not carry. Which language the answer is spoken in is chosen on the device from the answer's
own text, as above.

The audio level (Story 64.3, AD-186) never leaves the process either. The input tap that hands
every buffer to the recogniser also reads the same buffer's samples and computes an RMS in
place (`voice_macos.rs:1273-1289`; `voice_ios.rs:1355-1370`); a meter in the core smooths it
and lets at most one reading through every 40 ms, only when it moved
(`keeper-core/src/voice/level.rs:52-59`, `:90`). What crosses to the webview is one number in
`0.0..=1.0` on the existing voice channel — never samples, never a buffer — and nothing is
recorded or written to disk on the way. The recogniser still receives every buffer unchanged,
before the meter looks at it.

### The phone's record and the island add no egress

Epic 65 adds nothing to the table above (AD-196): the voice port's record is a bounded ring in
memory (`keeper-core/src/voice/events.rs:8-9`, `:26` — 200 entries, nothing written to disk,
read back by the `voice_events` command in `src-tauri/crates/keeper/src/voice_log.rs:81` and
shown under Settings › Agents & voice › *Voice* on the phone with the sentence "Kept in memory
only — nothing is written or sent", `src/components/settings/voice-section.tsx:46-47`), and the
Live Activity
is started, updated and ended from the app process with `pushType: nil`
(`src-tauri/crates/keeper/gen/apple/Sources/keeper/KeeperIsland.swift:76`) — no APNs push
for the activity and no push token for it; the app's push entitlement and App Group (Epic 98)
are for push — agents' approval requests and your own agent's answers, through the gateway row
above (`docs/ios.md` § *Push*) — never the island — so nothing about what the phone heard leaves
the device
(Apple, `Activity.request(attributes:content:pushType:)`,
`https://developer.apple.com/documentation/activitykit/activity`, and *Displaying live data
with Live Activities*, both read 2026-09-05; Epic 65, AD-194, AD-196). The `voice_on_device`
source scan described above picks up every `voice*.rs` in the shell crate by prefix
(`keeper-core/tests/voice_on_device.rs:38-56`), so the island's Rust side (`voice_island.rs`)
is inside the network-API scan without being named.

## The observability consent boundary

Epic 71 replaces the former unconditional no-telemetry claim with **no PostHog
collection or contact without the relevant local consent**. New and existing
installations default off. Rust owns consent, schema validation and normal-app
export; since epic 102 diagnostics and product statistics are synchronized account
settings (R462, below). Settings → About
discloses the configured ingestion host from the same live policy. The management
host `us.posthog.com` is used only by separate maintainer tooling, not by the app.

| Consent | What may leave | What it cannot enable |
| --- | --- | --- |
| Diagnostics | Closed readiness/interaction durations, a closed microphone-loss cause per recording (`removed`, `stalled` or `silent`), content-free operation traces, and — since epic 102 (R461) — every `WARN`/`ERROR` line keeper logs and every error the webview reports, as a `$exception` with level, target, file:line, a fingerprint and the message **redacted** (Matrix ids, emails, URL paths and queries, home-relative paths, hex runs ≥ 16, base64 runs ≥ 24, `syt_`/`phc_`/bearer tokens and values after `token=`/`key=`/`password=` removed; at most 1,000 characters); a line under an agent target leaves without its message | Unredacted text, stack payloads or a line's raw fields; an agent line's message or fields; the microphone's name or anything recorded |
| Product statistics | Closed events for opening the command palette or settings | Search text, command names, setting values, person profiles or account identifiers |
| Remote configuration | A request for the public `keeper-client-config` flag payload and a bounded support sentence | Collection, consent, security policy, destinations, grants, encryption changes or synchronized user preferences |
| Synthetic usability study | Replay/heatmaps of a separate synthetic surface after explicit per-session start | Normal-app DOM or real account, drive, note, agent or recording content |

Normal observations carry a random installation identifier, never a Matrix ID
or email, and no implicit cross-device identity. Payloads exclude messages,
filenames, local paths, notes, credentials, recordings, agent conversations,
normal-app URLs, arbitrary DOM attributes, console values and network bodies.
Recording sends no content and adds no destination, and on-device speech
guarantees are unchanged; the one recording signal is the closed microphone-loss
cause below, under diagnostics consent.

**Epic 102 (R461, R462) amends the above.** Diagnostics and product statistics
are now the account's synchronized settings (`telemetry.diagnostics`,
`telemetry.product_analytics`): on or off on one device is on or off on every
device of the account; a settings file pinning one wins over a local save,
and every account-layer change takes effect at once (R495, R496). An epic-71
per-installation "on" is adopted only once the account's settings are known
on the device, and only where the account has no value. Remote configuration and the installation
identifier stay per installation. Under diagnostics consent the redacted
problem records above leave; the destination is the same ingestion host. The
payload rules of the previous paragraph hold for everything else; for a
problem, "messages, filenames, local paths" means what survives the redactor:
text between the redacted spans, and paths that are not home-relative. Whole
rule and limits: [docs/observability.md](observability.md#telemetry-posthog-opt-in).

**Epic 100 (R381, as amended by R387) added** a closed `surface` property on
`keeper_interaction` — `slash_menu_opened`, `slash_menu_chosen`,
`launch_default_agent` — never the command, its arguments, the room or the
agent. Its rule that a panic is never counted is replaced by R461: a panic's
`ERROR` line leaves like any other, redacted. What is logged where, and what
leaves the device, is [docs/observability.md](observability.md).

**A lost microphone adds one closed count and nothing else:** when a recording
loses its microphone, the app shell sends `keeper_recording_mic_lost` with one
`cause` property — `removed` (unplugged, or its fallback failed), `stalled` (the
feed stopped) or `silent` (only digital silence arrived) — at most once per cause
per session, and only with diagnostics consent. It carries no duration, no
device name, no warning message, no path and nothing recorded; the destination
is the PostHog host already listed, and the recording sidecar itself still
contacts no network host.

**Transport still reveals metadata.** Direct HTTPS necessarily exposes the
client's source IP, request timing and ordinary connection metadata to PostHog
and its serving infrastructure. Disabling GeoIP/person enrichment does not
hide the IP from the transport. These are minimized content-free observations,
not a claim of anonymous networking; public project tokens are forgeable.

Admission is bounded to **60 admissions per minute**, not 60 observations:
one admission may generate multiple observations. The normal Rust exporter
uses a lazily created pooled HTTP client, at most three concurrent collector
requests plus one flags request, bounded deadlines/queues and no retries.
These exporter bounds do not describe the synthetic study SDK. Offline,
revoked and dropped observations are missing; opt-in counts are not all
installations or all people, and app readiness is not a claim of browser web
vitals or total Rust startup coverage.

Revocation stops future collection, discards queued unsent records and cancels
in-flight work as practicable. Bytes already transmitted cannot be recalled
by a local switch. A study has a separate session-local opt-in: real application
content is unmounted before study capture starts, and capture/outgoing study
requests stop before the real application returns. Ordinary diagnostics never
enable replay. Synthetic study observations are separate from production
metrics and must not inflate them.

No AI processing or Replay Vision is automatically approved. Metric proposals
and any future AI-analysis scope need separate human review; authoring a proposed
metric with an AI tool is not permission for PostHog to process user content with AI.
The provisioning workflow neither autoapproves metrics nor enables person enrichment.
See [the maintainer runbook](credentials.md#posthog-maintainer-runbook).

Any new destination or changed collection boundary must be reflected here and
in the live policy, and will surface in the release egress diff note.

## The update endpoint is a shared constant

The update endpoint appears in exactly two places, which must stay in sync:

- `src-tauri/crates/keeper/tauri.conf.json` → `plugins.updater.endpoints`
- `keeper-core::egress::EGRESS_UPDATE_ENDPOINT` (the value the egress list shows)

Changing the release repository or endpoint means changing both — and a unit test
(`egress_update_endpoint_matches_tauri_conf` in the `keeper` crate) fails the build if
they diverge, so the disclosed update host can never silently drift from the one the
updater actually checks. Likewise the `api.beeper.com` host is single-sourced from
`keeper-core::auth::BEEPER_API_BASE` (the same constant the Beeper login flow uses).

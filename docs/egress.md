# Egress surface

keeper is a **client only** — it has no server-side components and phones home to nothing it
does not have to. This document is the canonical, diffable record of every network destination
keeper contacts, and it is enforced in two ways:

1. **In the app.** Settings → About renders the *live* egress list, computed in Rust from your
   actual signed-in accounts and your actual folder-sync profiles (`egress::compute_egress`,
   wired through the `egress_list` command). The *set* of entries is derived from the same
   accounts registry the session-restore path reads and the same profile set the sync engine
   drives (never a hand-maintained list), so it can never drift from which accounts and folders
   you actually have; the `api.beeper.com` and update-endpoint hosts are fixed constants
   surfaced by that live state (and single-sourced — see below).
2. **In releases.** The release workflow emits a per-release "Egress diff note" that diffs this
   file against the previous tag into the job summary (NFR-11, AD-23), so any change to where
   keeper sends traffic is visible on every release.

## What keeper connects to

| Destination | When | Why |
| --- | --- | --- |
| Each account's **Matrix homeserver** (e.g. `https://matrix.example.org`) | One entry per distinct homeserver you are signed into (duplicates collapse to one) | All Matrix protocol traffic — sync, sending, media, key backup, verification. |
| **`api.beeper.com`** | Only when at least one account is a Beeper account (by provider tag **or** by homeserver host `matrix.beeper.com`) | Beeper's unofficial email-code login and account service. Appears exactly once. |
| Each sync profile's **git remote host** (e.g. `github.com`, `forgejo.example.org`) | One entry per distinct remote *host* across your folder-sync profiles (duplicates collapse to one); absent entirely for a profile whose remote is a local path or a pendrive, which reaches no network | Fetch, push and LFS transfer for that folder. Only the **host** is shown — never the repository path, the username, or a credential that a badly-stored profile put in the URL. See `egress::remote_host`. |
| Each configured **AI provider's host** (e.g. `localhost`, `127.0.0.1`, `gw.example.org`) | One entry per distinct provider *host* across your Bots providers (duplicates collapse to one); absent entirely when no provider is configured, which is how keeper ships | Chat completions, model and capability discovery, and a bot probe against the Hermes or Ollama endpoint you typed. Only the **host** is shown — never the `/v1` path, a profile prefix, or a credential. See the provider chapter below, and `egress::remote_host`. |
| **`github.com/tgorka/keeper/releases/...`** (the signed-update `latest.json` endpoint) | Always (an update check) | Signed auto-updates (NFR-12). Downloads are cryptographically verified against keeper's minisign public key before installing. |
| **`*.githubusercontent.com`** (GitHub's release-asset CDN) | Only while downloading an update the user chose to install | GitHub serves release files (the update binary) from its content-delivery network, which the `github.com` release URL redirects to. Disclosed so the egress list is exhaustive, not just the check endpoint. |

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

## An AI provider is a destination you typed, disclosed as a host

The Bots surface (⌘9, Epic 61) talks to a model over the OpenAI-compatible wire, and a model
lives at an endpoint. That endpoint is the one destination in this document that **exists only
because you typed it**: keeper ships no default endpoint, no hosted model and no proxy of its own,
so a fresh install has no provider row here at all, and adding one in Settings → Bots is the act
that creates the destination. Removing the provider removes the row on the next open, with no
cache to go stale. This is the same posture the no-telemetry section below states for everything
else — *"keeper never sends your data, usage, or diagnostics anywhere except the servers listed
above"* — and the provider row is how a model endpoint becomes one of the servers listed above
rather than an exception to it. The reasoning is recorded as `docs/decisions.md` D-4.

The row is **derived, never hand-written**. `EgressKind::BotProvider` is computed by
`egress::compute_egress` from `bots::store::provider_base_urls`, read from the same store the
chat itself reads, exactly as the git-remote rows are read from the sync profiles the engine
drives (AD-53). And it is reduced by the **same** `egress::remote_host` a git remote goes
through — not a second reduction that would have to be kept in step with it — so the list shows
`gw.example.org`, not `https://gw.example.org/v1` and not `/p/<profile>/v1`. The base-URL grammar
in `bots::url` already refuses userinfo and a query string, so a credential cannot legally reach
this function from a provider row; it is stripped anyway, because Settings → About is a screen
people share and the row that leaks a token is the one written by a build whose grammar was
looser than this one. Two providers on one host are one entry, for the reason the sync-remote
chapter gives: they are one destination.

**A loopback or private-network host is the normal case here, and it is disclosed, not hidden.**
Ollama's documentation points every user at `http://localhost:11434`; Hermes' api-server binds
`127.0.0.1:8642` by default. A blocklist over private ranges — the standard SSRF advice for a
server accepting arbitrary URLs — would reject the two endpoints the feature exists to reach, so
keeper answers the SSRF question the way it answers every other reach-outside question: by
disclosure plus an explicit user act. `bots::url::parse_base_url` accepts a loopback, private or
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

## Screen recording adds no egress

Screen recording (the macOS recording phase, Epics 16–20) is fully local: the `keeper-rec`
capture sidecar and the recording UI contact **no network host**, and there is no upload,
share-link, transcription, or cloud affordance anywhere in the recording feature — recordings
only ever land in the local destination folder. The per-release egress inventory diff for the
recording phase is therefore empty. Like the update endpoint, this is enforced by tests:
source-scan audits fail the build if a network API ever appears in the sidecar's Swift sources
(`keeper_rec_sidecar_sources_are_network_free` in the `keeper` crate) or an egress affordance
in the recording frontend (`zero-egress.test.ts`).

## Voice adds no egress

Talk mode on the phone (Epic 62) turns speech into text, sends that text to the bot as an
ordinary message, and speaks the answer — and none of the three steps contacts a network host
the table above does not already name. The message goes to the provider row you typed, exactly
as a typed message would. Recognition is Apple's on-device recogniser: every request the iOS
port builds sets `requiresOnDeviceRecognition = true`, and the port checks the recogniser's
`supportsOnDeviceRecognition` before it builds one. Apple's recogniser will otherwise fall back
to Apple's servers, and that fallback would be a destination this file does not name — so
there is no server-fallback path in the code, not a disabled one. Synthesis is the system's own
voice, which needs nothing from the network. The wake phrase is matched in the same on-device
transcript and never leaves the device; it is stored as a setting, not sent anywhere.

When the phone has no on-device model for its locale, keeper says so — *"on-device speech
recognition for `pl_PL` is not on this phone — download that language under Settings > General >
Keyboard > Dictation Languages; keeper never sends your voice to a server"* — and does nothing
else. That sentence is not a fallback, it is the refusal: the person is told which language to
download and why keeper will not simply use the network instead. Like the sidecar audit above,
this is enforced by a source scan rather than asserted: `voice_on_device` in `keeper-core`'s
tests reads the voice modules of both crates off disk and fails the build if a recognition
request is ever built without the on-device flag, if the flag is ever set to `false`, or if a
network API appears in any voice module (NFR-50). The decision is recorded as
`docs/decisions.md` D-5.

## The no-telemetry invariant

keeper has **no telemetry, analytics, or crash reporting** — and no opt-in scaffolding for any of
it, because there is nothing to opt into. keeper never sends your data, usage, or diagnostics
anywhere except the servers listed above. This is a hard invariant: any change that would add a
new egress destination must be reflected here (and will surface in the release egress diff note).

## The update endpoint is a shared constant

The update endpoint appears in exactly two places, which must stay in sync:

- `src-tauri/crates/keeper/tauri.conf.json` → `plugins.updater.endpoints`
- `keeper-core::egress::EGRESS_UPDATE_ENDPOINT` (the value the egress list shows)

Changing the release repository or endpoint means changing both — and a unit test
(`egress_update_endpoint_matches_tauri_conf` in the `keeper` crate) fails the build if
they diverge, so the disclosed update host can never silently drift from the one the
updater actually checks. Likewise the `api.beeper.com` host is single-sourced from
`keeper-core::auth::BEEPER_API_BASE` (the same constant the Beeper login flow uses).

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

## Screen recording adds no egress

Screen recording (the macOS recording phase, Epics 16–20) is fully local: the `keeper-rec`
capture sidecar and the recording UI contact **no network host**, and there is no upload,
share-link, transcription, or cloud affordance anywhere in the recording feature — recordings
only ever land in the local destination folder. The per-release egress inventory diff for the
recording phase is therefore empty. Like the update endpoint, this is enforced by tests:
source-scan audits fail the build if a network API ever appears in the sidecar's Swift sources
(`keeper_rec_sidecar_sources_are_network_free` in the `keeper` crate) or an egress affordance
in the recording frontend (`zero-egress.test.ts`).

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

# Egress surface

keeper is a **client only** — it has no server-side components and phones home to nothing it
does not have to. This document is the canonical, diffable record of every network destination
keeper contacts, and it is enforced in two ways:

1. **In the app.** Settings → About renders the *live* egress list, computed in Rust from your
   actual signed-in accounts (`egress::compute_egress`, wired through the `egress_list` command).
   The *set* of entries is derived from the same accounts registry the session-restore
   path reads (never a hand-maintained list), so it can never drift from which accounts
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
| **`github.com/tgorka/keeper/releases/...`** (the signed-update `latest.json` endpoint) | Always (an update check) | Signed auto-updates (NFR-12). Downloads are cryptographically verified against keeper's minisign public key before installing. |
| **`*.githubusercontent.com`** (GitHub's release-asset CDN) | Only while downloading an update the user chose to install | GitHub serves release files (the update binary) from its content-delivery network, which the `github.com` release URL redirects to. Disclosed so the egress list is exhaustive, not just the check endpoint. |

## Bridges add no distinct egress

Bridges (WhatsApp, Telegram, Signal, …) are Matrix **appservices** that run **server-side**,
reached *through* the homeserver. keeper's client talks to the homeserver, and the homeserver
talks to the bridge — so a bridge adds no distinct client egress. The homeserver entry already
covers it. keeper never contacts a per-bridge host directly, and the egress list never fabricates
one.

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

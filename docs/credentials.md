# Credentials

Keeper is an open-source repository. **No credentials, tokens, or homeserver session data are
ever committed.** Development credentials live in 1Password and are read with the
[`op` CLI](https://developer.1password.com/docs/cli/).

## Dev test accounts

A 1Password item named **`keeper dev matrix`** (vault: `Private`) holds the Matrix test
account used for local development and manual testing:

| Field        | Meaning                                             |
| ------------ | --------------------------------------------------- |
| `homeserver` | Homeserver URL, e.g. `https://matrix.example.org`   |
| `username`   | Full Matrix user id, e.g. `@keeper-dev:example.org` |
| `password`   | Account password                                    |

Read values ad hoc:

```sh
op item get "keeper dev matrix" --fields label=homeserver
op item get "keeper dev matrix" --fields label=username
op item get "keeper dev matrix" --reveal --fields label=password
```

Or inject into a dev shell via `op run`:

```sh
op run --env-file=.env.1p -- bun run tauri:dev
```

with `.env.1p` (committed, contains only `op://` references, no secrets):

```
KEEPER_DEV_HOMESERVER=op://Private/keeper dev matrix/homeserver
KEEPER_DEV_USERNAME=op://Private/keeper dev matrix/username
KEEPER_DEV_PASSWORD=op://Private/keeper dev matrix/password
```

## Beeper accounts

For testing against Beeper, log in inside the app itself (email + code, or app password).
Session tokens are stored by the app in the OS keychain — never in the repo.

## Runtime storage

The app stores Matrix session material (access tokens, E2EE keys) in:

- macOS Keychain for secrets (via Tauri/keyring)
- the app data directory (`~/Library/Application Support/dev.tgorka.keeper/`) for the
  encrypted matrix-rust-sdk store

Both are outside the repository.

## PostHog: public clients and private administration

Keeper's PostHog project is **keeper**, project ID `605256`, in US Cloud.
The management API is `https://us.posthog.com`; client ingestion uses
`https://us.i.posthog.com`. The vault's `hostname` field names the management
host, not the ingestion host.

| Purpose | 1Password reference | May ship in an app? |
| --- | --- | --- |
| Project ingestion token (`phc_`) | `op://tg/keeper-posthog-project-api-key/credential` | Yes: public, never an authorization boundary |
| Personal management key | `op://tg/keeper-posthog-personal-api-key/credential` | **Never** |
| Project ID | `op://tg/keeper-posthog-project-api-key/project-id` | Yes |

For a locally built client, inject only the public configuration:

```sh
op run --env-file=deploy/posthog/client.env.1p -- \
  env -u OP_SERVICE_ACCOUNT_TOKEN -u OP_CONNECT_TOKEN \
      -u GH_TOKEN -u GITHUB_TOKEN -u POSTHOG_PERSONAL_API_KEY \
      bun run tauri:build
```

`deploy/posthog/client.env.1p` is tracked and contains only public references,
not credential values. The obsolete root `.env.posthog.1p` stays ignored.
Never substitute `maintainer.env.1p` for the client template: it injects a
personal key. Do not add a personal key, secure feature-flags key, or
`OP_SERVICE_ACCOUNT_TOKEN` to client configuration. A distributed Rust binary
is just as public as JavaScript.

`op run` inherits its parent's environment: a public-only template does not
remove the 1Password service credential or GitHub token already in the shell.
The command above removes them before the build. Also remove any legacy
`OP_SESSION_*` variables from the invoking shell; never build inside the
maintainer command's credential-bearing environment.

GitHub build variables `KEEPER_POSTHOG_HOST` and
`KEEPER_POSTHOG_PROJECT_TOKEN` contain only public client configuration.
`POSTHOG_PERSONAL_API_KEY` is an existing GitHub **secret**, reserved for
trusted maintainer provisioning. The real key must not be made available to
build, test, release-asset creation, or pull-request code. Never expose it
through a `VITE_` variable or write it to `$GITHUB_ENV` for subsequent steps.
CI deliberately supplies a **synthetic noncredential** under that variable
name to its app build; the artifact scanner must find no copy of the canary.

The artifact check `bun scripts/check-client-secrets.ts <artifact> [...]`
rejects recognizable privileged-key material without printing matching bytes.
Every requested root must contain a regular file; missing, empty and
symlink-only roots fail. Symlinks are not followed. It detects PostHog
personal/secure keys, encoded 1Password service-account capsules, GitHub and
Matrix tokens, PEM/minisign private-key headers, and the synthetic canary.
It also checks privileged credential values present in the scanner's environment,
including `OP_SESSION_*`; updater signing material is checked both verbatim and
base64-decoded.
This is defense in depth, not a proof that arbitrary unknown credentials
cannot leak. Key separation is the primary control.

Release CI runs the scanner inside the Tauri build runner, **before**
tauri-action uploads anything: `dist`, the actual uncompressed `keeper.app`
tree, and the required `Contents/MacOS/{keeper,keeper-rec}` executables.
Each syncd job scans its packaged executable before upload and cannot run
after the app release job fails. The scanner does not unpack or claim to scan
compressed DMG/tar payloads. See [release checks](release.md#observability-and-credential-release-gates).
Draft publication remains a human decision; a repository owner can manually
publish despite a failed check, which this workflow cannot prevent.

## PostHog maintainer runbook

Only run this tooling from reviewed code. `deploy/posthog/manifest.json` owns
project `605256` (`keeper`), never tgsite. The tool manages event definitions,
the public `keeper-client-config` flag, behavioral cohorts, a private dashboard,
four proposed metrics with insights and callable Endpoints, and a settings
engagement conversion goal. Campaign-source mappings remain documented
boundaries, not live ad-account integrations.

API references: [event definition creation](https://posthog.com/docs/open-api-spec/event_definitions_create.md)
supports the managed description/tags; [metric creation](https://posthog.com/docs/open-api-spec/data_catalog_metrics_create.md)
and [metric PATCH](https://posthog.com/docs/open-api-spec/data_catalog_metrics_partial_update.md)
both permit `confidence: null`. These schemas support the implementation,
but live readback remains the deployment gate.

### Trusted workflow configuration

`.github/workflows/posthog.yml` is manual (`workflow_dispatch`) on
`tgorka/keeper`'s `main` only, with read-only repository permissions, reviewed
checkout, no dependency installation/build/cache, and one serialized
`posthog-keeper-605256` concurrency group. It uses:

| Workflow environment variable | Existing source |
| --- | --- |
| `POSTHOG_HOST` | Literal `https://us.posthog.com` |
| `POSTHOG_PROJECT_ID` | Literal `605256` |
| `POSTHOG_INGEST_HOST` | Public Actions variable `KEEPER_POSTHOG_HOST` (`https://us.i.posthog.com`) |
| `POSTHOG_PROJECT_API_KEY` | Public Actions variable `KEEPER_POSTHOG_PROJECT_TOKEN` (`phc_`) |
| `POSTHOG_PERSONAL_API_KEY` | Existing Actions secret of the same name |

Do not create duplicate `POSTHOG_*` variables or another project-token secret.
The repository admin must configure the **`posthog-maintainers` protected
environment**, require approval from `tgorka` (GitHub user ID `1956779`), and
add exactly one custom deployment policy: branch `main`, not a tag or wildcard.
Disable administrator bypass in GitHub's environment settings. A sole maintainer
may explicitly approve their own run; preventing self-review requires another
reviewer and a corresponding reviewed update to `scripts/posthog/environment.mjs`.

A separate, secret-free workflow job reads the actual reviewer and branch
policies before the personal-key job can start. Missing protection, failed API
lookup, an unreviewed reviewer, administrator bypass enabled or unreported, or
a broader deployment policy fails closed. A YAML environment name alone
confers no protection.
Reuse the existing secret rather than copying it into another environment.
Protection gates this job's use, not every possible use of a repository secret:
review future workflow changes and repository access too.

### Review, plan, apply, verify

1. Review the manifest **and provisioning code diff** at the exact commit.
   Review every SQL query's tables, projections, joins, subqueries/UNIONs,
   consent category, synthetic exclusion, time window and aggregation grain.
   The SQL substring validator is **sanity only**, not a parser, privacy proof
   or authorization boundary. Human SQL review plus the exact reviewed
   manifest SHA-256 is the mutation control; a correct substring can occur
   in a comment or in only one branch of an unsafe query.
2. Run the offline dry run without any credential:

   ```sh
   POSTHOG_HOST=https://us.posthog.com POSTHOG_PROJECT_ID=605256 node scripts/posthog/cli.mjs
   ```

   Read `manifest sha256 <digest>`; do not use a digest copied from an old
   audit. Formatting changes the hash too.
3. For local maintainer commands only, use the **separate**
   `deploy/posthog/maintainer.env.1p` template. Never resolve it into logs,
   artifacts or a build environment. Read-only remote comparison:

   ```sh
   op run --env-file=deploy/posthog/maintainer.env.1p -- node scripts/posthog/cli.mjs --plan
   ```

4. After human review, run manual workflow mode `apply` with
   `reviewed_sha256` set to the digest from step 2. The equivalent local
   command (set `REVIEWED_SHA256` to that reviewed digest first) is:

   ```sh
   op run --env-file=deploy/posthog/maintainer.env.1p -- node scripts/posthog/cli.mjs --apply "--reviewed-sha256=$REVIEWED_SHA256"
   ```

   Apply finishes with readback verification. Run workflow mode `verify`
   separately, then repeat `apply`: every managed resource should be
   `unchanged`, with no duplicate resources or write churn.
5. On an error, preserve the safe names/status output, not response bodies
   or keys. Resolve ownership collisions, duplicate names, archived resources
   and approved-metric drift manually. There is no automatic write retry:
   a lost response may hide a completed write, so run `plan` before retrying.
   Do not run concurrent applies from different hosts. A local
   `keeper-posthog-605256.lock` directory in the OS temporary directory
   prevents concurrent writers; investigate the owning process before
   removing a stale lock. Tests use an isolated temporary directory.

### Synthetic smoke and governance

Run workflow mode `smoke` with the reviewed digest, or:

```sh
op run --env-file=deploy/posthog/maintainer.env.1p -- node scripts/posthog/cli.mjs --smoke --apply "--reviewed-sha256=$REVIEWED_SHA256"
```

This verifies managed resources first, fetches the public flag payload,
sends one synthetic `keeper_ops_smoke` event with a generated UUID and no
person profile, then checks that exact UUID through a count-only query and
runs all four Endpoints. It does not read raw event samples or export app
content. Observation is bounded to 12 attempts, each at most 15 seconds,
with 5-second gaps; a read-only query timeout consumes an attempt. Writes
and ingestion are never retried automatically. Endpoint caching is 900 seconds.

Live apply/readback and verification succeeded for all managed resources on
2026-09-12, including the explicit `confidence: null` reconciliation; synthetic
UUID readback and all four Endpoints also passed. This is evidence for that
reviewed manifest, not for later changes. Approved metrics remain protected
from automatic changes and require a human decision.
The project's replay ingestion was enabled only after the isolated study
boundary passed privacy checks. No Replay Vision/AI approval setting was
changed; starting a study does not grant separate AI-processing approval.

Metrics retain honest `ai_generated` provenance and the authoring model
`openai-codex/gpt-6-astra`; unknown/calibration-free confidence is null.
Automation never approves metrics or enables AI processing/Replay Vision.
A maintainer must separately review definitions, coverage and missing-client
bias before canonical approval. Personless installation metrics count
`distinct_id`, not humans; profile-based cohorts may remain empty. Never
enable person enrichment or cross-device identity to make a cohort appear
populated. The goal counts settings engagement, not revenue or customers.
Public ingestion tokens are forgeable, and Endpoints are maintainer queries,
not a client authorization system or a shipped Keeper server.

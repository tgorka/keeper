# Release runbook

keeper ships a Developer-ID-signed, hardened-runtime, Apple-notarized Apple Silicon
(`aarch64-apple-darwin`) `.dmg`. Releases are produced by the tag-triggered
`.github/workflows/release.yml` workflow, which uses
[`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action). All signing and
notarization material lives in GitHub Actions secrets — never in the repository.

> **Apple Silicon only.** The pipeline builds `aarch64-apple-darwin`; Intel (`x86_64`) and
> universal binaries are intentionally out of scope (macOS-first). Intel Macs are unsupported —
> this is by design, not a regression.

## Required GitHub secrets

Configure these under **Settings → Secrets and variables → Actions**.

### Developer ID signing

| Secret | What it is |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` certificate (the signing cert + private key). Export from Keychain, then `base64 -i cert.p12 \| pbcopy`. |
| `APPLE_CERTIFICATE_PASSWORD` | Password protecting the exported `.p12`. |
| `APPLE_SIGNING_IDENTITY` | The signing identity string, e.g. `Developer ID Application: Your Name (TEAMID)`. Tauri signs with this identity; it is intentionally **not** hardcoded in `tauri.conf.json`. |
| `KEYCHAIN_PASSWORD` | A password Tauri uses to create a temporary keychain on the runner to import the certificate. Any strong random value. |

### App Store Connect API-key notarization

| Secret | What it is |
| --- | --- |
| `APPLE_API_ISSUER` | App Store Connect API **Issuer ID** (a UUID from App Store Connect → Users and Access → Integrations → App Store Connect API). |
| `APPLE_API_KEY_ID` | The API **Key ID** for the notarization key. Passed to Tauri as the `APPLE_API_KEY` env var. |
| `APPLE_API_KEY_P8_BASE64` | Base64-encoded contents of the `.p8` private key file downloaded from App Store Connect (`base64 -i AuthKey_XXXX.p8 \| pbcopy`). The workflow decodes this to a file and points `APPLE_API_KEY_PATH` at it. Named distinctly from the `APPLE_API_KEY` env var (the Key ID) to avoid confusion. |

The API key must have at least the **Developer** role so it can submit for notarization.

## How the workflow maps secrets to Tauri env

Tauri v2's macOS signer reads these environment variables automatically:

- `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `KEYCHAIN_PASSWORD` → code signing.
- `APPLE_API_ISSUER`, `APPLE_API_KEY` (the **Key ID**), `APPLE_API_KEY_PATH` (path to the decoded `.p8`) → notarization.

A dedicated step base64-decodes the `APPLE_API_KEY_P8_BASE64` secret into
`$RUNNER_TEMP/api_key.p8` before the build, and `APPLE_API_KEY_PATH` points at that file. The
`APPLE_API_KEY` env var itself carries the **Key ID** (from the `APPLE_API_KEY_ID` secret) — the
two are deliberately named apart so the file secret and the Key-ID env var never collide.
Hardened runtime is applied automatically by Tauri's signer; no entitlements file is required
(WKWebView JIT runs in its own system-entitled process).

## How to cut a release

1. Ensure `main` is green (CI passes, including the license firewall).
2. Bump the version if needed in `src-tauri/crates/keeper/tauri.conf.json` and `package.json`.
3. Create and push a `v*` tag:

   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

4. The `Release` workflow triggers on the tag, builds on `macos-latest`, signs and
   notarizes the `aarch64-apple-darwin` `.dmg`, and creates a **draft** GitHub release
   carrying the artifact.
5. Review the draft release, then publish it.

## Release-time verification (requires the built artifacts)

Run these on the downloaded, notarized artifacts to confirm signing and stapling:

```sh
# Developer ID authority + hardened runtime flag on the app bundle.
codesign -dv --verbose=4 keeper.app
# Expect an "Authority=Developer ID Application: …" line and "flags=…(runtime)".

# Gatekeeper accepts the notarized dmg.
spctl -a -t open --context context:primary-signature keeper.dmg

# The notarization ticket is stapled to the app.
xcrun stapler validate keeper.app
```

## Required status checks (branch protection)

Required checks are enforced via repository **branch-protection settings**, not YAML. A repo
admin must, under **Settings → Branches → Branch protection rules** for `main`, enable
"Require status checks to pass before merging" and mark these CI checks as required:

- **License firewall** — `cargo deny check licenses bans sources` (Rust) and the JS license gate (`bun run check:licenses`).
- **Frontend** — biome lint, `tsc` typecheck, vitest.
- **Rust** — `rustfmt --check`, clippy `-D warnings`, cargo-nextest.
- **Tauri build** — `tauri build --no-bundle`.

These correspond to the `licenses`, `frontend`, `rust`, and `build` jobs in
`.github/workflows/ci.yml`. **CI is the license source of truth**: the JS gate scans the
installed `node_modules` tree, so run it via CI (which does a clean `bun install
--frozen-lockfile`) rather than trusting a local run against a possibly-stale tree. `bun run
check:all` includes `check:licenses` for local pre-flight, but the required CI check is what
gates merges.

> The cargo-deny gate runs `licenses bans sources`, not the bare `cargo deny check`: advisory
> (RUSTSEC) gating is deliberately excluded because the crate graph carries pre-existing
> `unmaintained` advisories for Linux-only gtk-rs GTK3 bindings that are irrelevant to this
> macOS-first app. Vulnerability/advisory gating is a separate, unstoried concern.

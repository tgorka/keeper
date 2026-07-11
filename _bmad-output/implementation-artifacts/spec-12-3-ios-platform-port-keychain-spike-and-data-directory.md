---
title: 'iOS Platform Port — Keychain Spike and Data Directory'
type: 'feature'
created: '2026-07-11'
status: 'done'
baseline_revision: '3324cf8c62dfd73cbb3c2e6e01f2831250f71cf1'
final_revision: 'a30381ed7835d1a3b96a9269128266f04dec6a14'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Story 12.2 gave `IosPlatform` (in `src-tauri/crates/keeper/src/ipc.rs`) keychain get/set/delete that reuse the exact same `keyring::Entry` calls as desktop. On iOS that lands session tokens at the keychain's default accessibility (`kSecAttrAccessibleWhenUnlocked`) with no this-device-only guarantee — not the AD-29 requirement (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`: readable by a resumed background sync loop, invisible to other apps, excluded from iCloud Keychain). No AD-29 spike verdict has been recorded, and `data_dir()`'s single-root invariant is undocumented.

**Approach:** Run the AD-29 keychain spike as a source-level investigation with a recorded verdict, and implement its outcome: `keyring`/`apple-native` cannot set the accessibility class, so switch **iOS only** to the contained fallback — direct `security-framework` generic-password calls that pin `AccessibleAfterFirstUnlockThisDeviceOnly` via a protection-only `SecAccessControl`, behind the *same* `Platform` port with every call site unchanged. Confirm and annotate `data_dir()` as the single app-container root. Desktop (macOS `keyring`) stays byte-identical. Runtime Simulator/on-device exercise folds into Story 12.6 per the epic.

## Boundaries & Constraints

**Always:**
- No token/secret/crypto byte crosses IPC or reaches JS-accessible storage or logs — keychain stays Rust-confined (NFR-9).
- iOS keychain items are created with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` (protection-only `SecAccessControl`, no user-presence flags) so the resumed background sync loop reads them headless with no auth prompt, they never sync to iCloud Keychain (this-device-only), and are invisible to other apps.
- The `Platform` trait signatures and all keychain/`data_dir` call sites (`auth.rs`) stay unchanged — this is a contained port swap.
- `data_dir()` resolves to one app-container root (`{container}/Library/Application Support/dev.tgorka.keeper`) with all account state (`accounts/<ulid>/sdk`, `keeper.db`, `archive.db`) under it — a future App Group move is a path change, not a migration.
- Desktop build byte-identical: `DesktopPlatform`/macOS keychain untouched; the new dep is iOS-target-gated so `Cargo.lock` and the desktop dep tree do not change.
- No `unsafe` (`unsafe_code = "deny"`) — use security-framework's safe wrappers only. No `.unwrap()`/`.expect()` in production paths; `?` + `PlatformError`.
- New dep passes cargo-deny (security-framework is MIT/Apache-2.0, already in-tree via keyring). Commit on the current branch; no team-ids/secrets in the repo.

**Block If:**
- The `AfterFirstUnlockThisDeviceOnly` accessibility cannot be met through a *safe* security-framework API (would force an `unsafe` exception or a new AGPL/GPL dependency).
- Existing account state is discovered to already live under multiple roots, so satisfying the single-root invariant would require a data migration rather than a path assertion.

**Never:**
- Do not change `DesktopPlatform` / macOS keychain behavior, or the `Platform` trait shape.
- Do not implement `NSFileProtectionCompleteUntilFirstUserAuthentication` / `isExcludedFromBackup` here — those are Epic 14 / Story 14.7.
- Do not add biometric/passcode/user-presence flags to the access control (flags = `0`) — the session must be readable by a headless resumed sync loop.
- Do not treat a headless Simulator boot as the exit gate — runtime keychain confirmation folds into Story 12.6.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| set new secret | key absent | item created carrying `AfterFirstUnlockThisDeviceOnly` AC; `Ok(())` | any SF error → `PlatformError::Keychain` |
| set existing key | key present | prior item deleted then re-added, protection class re-applied; `Ok(())` | SF error → `PlatformError::Keychain` |
| get present | key present, device after first unlock | `Ok(Some(value))`, no auth prompt | SF error (≠ NotFound) → `PlatformError::Keychain` |
| get missing | key absent | `Ok(None)` | `errSecItemNotFound` mapped to `None`, not an error |
| delete present | key present | item removed; `Ok(())` | SF error → `PlatformError::Keychain` |
| delete missing | key absent | `Ok(())` | `errSecItemNotFound` tolerated |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` — `#[cfg(target_os = "ios")] IosPlatform` impl (`:491-574`). Rewrite `keychain_set/get/delete` (`:505-532`) onto `security_framework::passwords`; confirm/annotate `data_dir` (`:496-503`) single-root. `KEYCHAIN_SERVICE` const (`:254`) shared unchanged. `keyring::Entry` is referenced only inline inside `DesktopPlatform` (`:376`–`:400`, `#[cfg(desktop)]`, no top-level `use keyring`), so the iOS build ends up with zero keyring reference — no unused-import lint. `DesktopPlatform` (`:363-480`) stays byte-identical.
- `src-tauri/crates/keeper/Cargo.toml` — add a new `[target.'cfg(target_os = "ios")'.dependencies]` table (sibling of the existing desktop table at `:51`) with `security-framework = { workspace = true }`; keep `keyring` (`:36`) as-is for desktop.
- `src-tauri/Cargo.toml` — add `security-framework = "3"` to `[workspace.dependencies]` (`:20`-`:85` region); resolves to the 3.7.0 already in the lock via keyring — catalog entry only, no new lock line.
- `src-tauri/crates/keeper-core/src/platform.rs` — `Platform` trait + `keychain_*`/`data_dir` signatures (`:19-63`); unchanged (asserts the contained-fallback invariant).
- `src-tauri/crates/keeper-core/src/error.rs` — `PlatformError::Keychain(String)` reused for SF error mapping.
- `src-tauri/crates/keeper-core/src/auth.rs` — keychain call sites (`:463,:466,:567,:601,:665,:767,:774`) must remain unchanged (proves "same port, call sites unchanged").

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` -- add `security-framework = "3"` to `[workspace.dependencies]` (version catalog only; resolves to the 3.7.0 already present via keyring, so the desktop lock/tree is unchanged).
- [x] `src-tauri/crates/keeper/Cargo.toml` -- add an iOS-only `[target.'cfg(target_os = "ios")'.dependencies]` table with `security-framework = { workspace = true }`, plus a comment recording the AD-29 verdict (keyring cannot set keychain accessibility → contained security-framework fallback). Leave `keyring` for desktop.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- rewrite `IosPlatform::{keychain_set, keychain_get, keychain_delete}` to use `security_framework::passwords::{set_generic_password_options, get_generic_password, delete_generic_password}` + `PasswordOptions::new_generic_password(KEYCHAIN_SERVICE, key)` with `set_access_control(SecAccessControl::create_with_protection(Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly), 0)?)`. `keychain_set` deletes any prior item before adding (so the fresh `SecItemAdd` carries the protection class). Map `errSecItemNotFound` → `Ok(None)` in `get` / tolerated in `delete`; every other SF error → `PlatformError::Keychain(...)`. No `unsafe`, no `.unwrap()`/`.expect()`. Add a doc comment on `IosPlatform::data_dir` stating it is the single app-container root (future App Group = path change, not migration). Do not touch `DesktopPlatform` or the trait.

**Acceptance Criteria:**
- Given the whole workspace, when `cargo check --target aarch64-apple-ios` and `cargo clippy --target aarch64-apple-ios -- -D warnings` run from `src-tauri/`, then both finish clean (the iOS keychain rewrite compiles, no `unsafe`, no unused-import/dead-code warnings).
- Given a desktop build, when `bun run check:all` runs, then it is fully green and byte-identical — `DesktopPlatform`/macOS keychain unchanged — and `git diff src-tauri/Cargo.lock` shows no change (security-framework was already resolved).
- Given `keeper-core`, when greped, then no `cfg(target_os)` appears in core business logic, the `Platform` trait is unchanged, and `git diff src-tauri/crates/keeper-core/src/auth.rs` is empty (call sites unchanged — contained fallback).
- Given the AD-29 spike, then this spec's Design Notes record the verdict: keyring/apple-native cannot set `kSecAttrAccessible*` (its iOS backend calls `set_generic_password` with no options and exposes no accessibility API), so iOS switches to the contained security-framework fallback pinning `AfterFirstUnlockThisDeviceOnly`.
- Given `keychain_set` on iOS, then items carry a protection-only `SecAccessControl` for `AccessibleAfterFirstUnlockThisDeviceOnly` (no user-presence flags): readable headless by a resumed sync loop, this-device-only (never iCloud-synced), invisible to other apps. Runtime confirmation — set/get/delete round-trip and relaunch session-restore in Simulator/on-device under free signing, with no token in logs or across IPC — folds into Story 12.6 per the epic; the enforceable exit gate here is the iOS compile + clippy.

## Spec Change Log

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 0
- reject: 8
- addressed_findings:
  - `[low]` `[patch]` `src-tauri/crates/keeper/src/ipc.rs` (`ERR_SEC_ITEM_NOT_FOUND` doc) — the comment claimed importing the constant "would pull in the extra `-sys` crate", but `security-framework-sys` is already in the tree transitively; reworded to note that using it would only require declaring a *direct* `-sys` dependency, and that the value is a stable ABI-fixed Apple `OSStatus`. Code (the hardcoded const) unchanged and re-verified.
  - `[low]` `[patch]` `src-tauri/crates/keeper/src/ipc.rs` (`keychain_set`) — documented the delete-then-add atomicity trade-off: the design is deliberate (avoids the fragile `kSecAttrAccessControl`-in-update-query path) and safe here because keychain keys are write-once per account and a mid-sequence failure surfaces as an `Err` degrading to re-login, never silent corruption. Comment-only.
  - `[low]` `[patch]` `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` — the "no desktop lock/tree churn" comments overstated: the lock does gain one `security-framework 3.7.0` dependency edge on the `keeper` crate. Reworded to "no new crate *version* pulled; the only lock change is the dependency edge; desktop tree/build byte-identical." Comment-only.

Rejected (8, from the general reviewer; Edge Case Hunter found none): missing `kSecUseDataProtectionKeychain`/store-divergence/orphaning (refuted — iOS has a single data-protection keychain, the flag is a macOS-only toggle; keyring's `apple-native` backend uses the *same* `security_framework::passwords` module, so no divergence and no orphaning); "verdict asserted, not demonstrated" (substantiated in Design Notes with the keyring `ios.rs` citation — reviewer saw only the diff); non-atomic delete-then-add as data loss (deliberate & documented — write-once keys, graceful re-login; the atomicity note was added as a patch above); manual delete redundant with the library's internal update (intentional, a swallowed no-op, avoids the AC-update path); UTF-8 decode as a new wedge (refuted — keyring's `get_password`/`decode_password` also errors on non-UTF-8; equivalent, and unreachable since all writers pass `&str`); missing keychain access-group note (out of scope — App Group is Epic 14+; no regression vs keyring's default group); zero test coverage on the cfg-ios branch (real keychain, no host test possible; spec Verification already folds the manual round-trip into Story 12.6, matching 12.1/12.2); `flags = 0` magic value (the adjacent comment documents "no user-presence"; `0` is idiomatic); data_dir duplication / repeated bundle-id literal (pre-existing from 12.2, not caused by this change, stable strings).

## Design Notes

**AD-29 spike verdict (recorded).** keyring 3.6.3's `apple-native` iOS backend (`src/ios.rs`) writes via `security_framework::passwords::set_generic_password(service, account, secret)` with **no options**, and `keyring::Entry` exposes no accessibility API — so "keep keyring as-is" cannot satisfy `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. **Verdict: switch iOS to the contained `security-framework` fallback** (same `Platform` port, call sites unchanged). `security-framework` 3.7.0 is already in the tree (pulled by keyring) and is MIT/Apache-2.0, so no new license-firewall or lock churn on desktop.

**Why access control, not the plain `kSecAttrAccessible` attribute.** security-framework's only *safe public* way to set the protection class is `PasswordOptions::set_access_control`; the plain-attribute path needs the `pub(crate)` `push_query` + a `-sys` constant + `unsafe`, which `unsafe_code = "deny"` forbids. A protection-only `SecAccessControl` (flags = `0`, no biometry/passcode/user-presence) is equivalent for our purpose: after-first-unlock, this-device-only, no auth prompt on read.

Golden shape (~9 lines):
```rust
fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
    let ac = SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly), 0)
        .map_err(|e| PlatformError::Keychain(format!("access control: {e}")))?;
    let mut opts = PasswordOptions::new_generic_password(KEYCHAIN_SERVICE, key);
    opts.set_access_control(ac);
    let _ = delete_generic_password(KEYCHAIN_SERVICE, key); // fresh add applies the class
    set_generic_password_options(value.as_bytes(), opts)
        .map_err(|e| PlatformError::Keychain(format!("store: {e}")))?;
    Ok(())
}
```

**Delete-then-add on set.** `set_generic_password_options` does add-then-`SecItemUpdate`-on-duplicate, and an update whose *match* query carries `kSecAttrAccessControl` is fragile. Deleting first guarantees a clean `SecItemAdd` that applies the protection class. `keychain_get`/`keychain_delete` query only by class+service+account (accessibility isn't part of the match), so they still find AC-protected items with no prompt.

**data_dir needs no functional change.** 12.2's `IosPlatform::data_dir` already returns `dirs::data_dir()/dev.tgorka.keeper`, which inside the iOS sandbox is `{container}/Library/Application Support/dev.tgorka.keeper` — the single root holding `accounts/<ulid>/sdk`, `keeper.db`, `archive.db`. This story only asserts/annotates that invariant. `NSFileProtection*` and `isExcludedFromBackup` are explicitly Epic 14 / Story 14.7, out of scope here.

## Verification

**Commands:**
- `cd src-tauri && cargo check --target aarch64-apple-ios` -- expected: `Finished` (iOS keychain rewrite compiles for the whole workspace).
- `cd src-tauri && cargo clippy --target aarch64-apple-ios -- -D warnings` -- expected: no warnings (no unused keyring reference, no dead code from the port swap).
- `bun run check:all` -- expected: green (biome + tsc + vitest, `cargo fmt`/clippy, cargo-nextest, JS license firewall) — desktop unchanged.
- `cd src-tauri && git diff --stat Cargo.lock` -- expected: empty (security-framework already resolved via keyring; no desktop lock churn).
- `cd src-tauri && cargo deny check` -- expected: pass (security-framework is MIT/Apache-2.0, already allowlisted).
- Guards: `git diff src-tauri/crates/keeper-core/src/auth.rs` empty; no `unsafe`/`.unwrap()` in the new iOS keychain code; no `cfg(target_os)` in `keeper-core/src`.

**Manual checks (fold into Story 12.6 — not the blocking gate):** In the Simulator/on-device (needs CocoaPods + XcodeGen + free signing), exercise `keychain_set`→`get`→`delete` and confirm relaunch session-restore without re-login, with no token in logs or across IPC. Not reliably headless-automatable; per the epic, on-device confirmation is Story 12.6. The enforceable exit gate here is the iOS `cargo check` + `clippy`.

## Auto Run Result

Status: done

**Summary of implemented change.** Story 12.3 runs the AD-29 keychain spike and ships its verdict. The spike (a source-level investigation) found that keyring 3.6.3's `apple-native` iOS backend writes via `security_framework::passwords::set_generic_password` with no options and exposes no accessibility API, so "keep keyring" cannot satisfy `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. Verdict: switch **iOS only** to a contained `security-framework` fallback behind the same `Platform` port. `IosPlatform::{keychain_set, keychain_get, keychain_delete}` now use `security_framework::passwords` with a protection-only `SecAccessControl` (`ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly`, flags = 0) — readable headless by a resumed sync loop, this-device-only (never iCloud-synced), invisible to other apps. `keychain_set` deletes any prior item before the authoritative add; `get`/`delete` map `errSecItemNotFound` to `Ok(None)`/tolerated. `IosPlatform::data_dir` gained a doc comment fixing it as the single app-container root (future App Group = path change, not migration). `DesktopPlatform` (macOS keyring) and the `Platform` trait are byte-identical; call sites in `keeper-core::auth` are unchanged.

**Files changed.**
- `src-tauri/Cargo.toml` — added `security-framework = "3"` to `[workspace.dependencies]` (version catalog; 3.7.0 already resolved via keyring).
- `src-tauri/crates/keeper/Cargo.toml` — new `[target.'cfg(target_os = "ios")'.dependencies]` table with `security-framework = { workspace = true }` and the AD-29 verdict comment; `keyring` kept for desktop.
- `src-tauri/crates/keeper/src/ipc.rs` — rewrote the three `IosPlatform` keychain methods onto `security-framework` (no `unsafe`, no `.unwrap()`/`.expect()`); added the `#[cfg(target_os = "ios")] ERR_SEC_ITEM_NOT_FOUND` const and the `data_dir` single-root doc comment.
- `src-tauri/Cargo.lock` — one dependency edge (`security-framework 3.7.0`) recorded on the `keeper` crate; no version/resolution change.

**Review findings breakdown.** Two adversarial reviewers (general Blind Hunter + Edge Case Hunter) at session model capability (Opus), run in parallel without prior context. Edge Case Hunter: 0 findings. Blind Hunter: 11 findings. Final triage: 0 intent_gap, 0 bad_spec, **3 low patches** (all comment-accuracy: the `-sys`-already-in-tree correction, the delete-then-add atomicity note, and the "no lock churn" → "no new crate version, one edge" correction), 0 defer, 8 reject (refuted or out-of-scope — including the macOS-only `kSecUseDataProtectionKeychain`/orphaning reasoning misapplied to iOS's single keychain, and a UTF-8-wedge claim refuted because keyring also errors on non-UTF-8). No behavior/logic changed in the patch pass — comments only. No repair loopback.

**Verification performed (independently re-run by the orchestrator).**
- `cd src-tauri && cargo check --target aarch64-apple-ios` → `Finished` (the story's exit gate). ✓
- `cd src-tauri && cargo clippy --target aarch64-apple-ios -- -D warnings` → `Finished`, zero warnings (re-run after the comment patches). ✓
- `bun run check:all` → green: biome, tsc, vitest, core-tauri-free guard, `cargo fmt --check` + desktop clippy, cargo-nextest **765 passed / 0 failed**, bindings check, JS license firewall (0 denied). ✓
- `cargo deny check` → `bans ok, licenses ok, sources ok`. `advisories FAILED` is **pre-existing and unrelated** (GTK3/gtk-rs unmaintained RUSTSEC advisories in the tauri Linux-desktop tree; no `security-framework` advisory) — the license firewall the story cares about passes.
- Guards: `git diff src-tauri/crates/keeper-core/src/auth.rs` empty; no `unsafe`/`.unwrap()`/`.expect()` in the new iOS code; no `cfg(target_os)` added to `keeper-core/src`; `DesktopPlatform` untouched.

**Residual risks.** (1) The new iOS keychain path is `#[cfg(target_os = "ios")]` and hits the real keychain, so it cannot be host-unit-tested and has no simulator CI yet — the runtime set/get/delete round-trip, accessibility-class confirmation, and relaunch session-restore under free signing fold into Story 12.6 per the epic; the enforceable exit gate here is the iOS compile + clippy. (2) `keychain_set`'s delete-then-add is non-atomic; safe here because keys are write-once per account and a mid-sequence failure degrades to re-login (documented in code), but a future multi-write key would want revisiting. (3) `ERR_SEC_ITEM_NOT_FOUND` is a hardcoded stable Apple `OSStatus` (`-25300`); ABI-fixed, but not compile-tied to the SDK constant. (4) `NSFileProtection*` / `isExcludedFromBackup` and any App Group keychain-sharing remain Epic 14 / Story 14.7, out of scope here.
